use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    Extension,
};
use std::sync::Arc;

use crate::{
    app_state::AppState,
    middleware::auth::AuthUser,
    services::supabase::Job,
};

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    jobs: Vec<Job>,
}

pub async fn dashboard(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
) -> Result<impl IntoResponse, StatusCode> {
    let jobs = state
        .supabase
        .get_jobs_for_user(&auth.user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let html = DashboardTemplate { jobs }
        .render()
        .unwrap_or_else(|e| format!("Template error: {e}"));
    Ok(Html(html))
}

/// htmx polling endpoint — returns a single <tr> partial for the given job.
pub async fn job_status(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let job = state
        .supabase
        .get_job(&job_id, &auth.user_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(axum::response::Html(render_job_row(&job)))
}

fn render_job_row(job: &Job) -> String {
    let action = match job.status.as_str() {
        "preview_ready" | "uploaded" | "preview_failed" => format!(
            r#"<a href="/api/jobs/{}/pay" class="btn">Pay &amp; Process</a>"#,
            job.id
        ),
        "completed" => format!(
            r#"<a href="/api/jobs/{}/download" class="btn">Download</a>"#,
            job.id
        ),
        "failed" => "<span>Failed</span>".to_string(),
        _ => format!("<span>{}</span>", job.status),
    };
    let preview_link = if job.preview_file_path.is_some() {
        format!(
            r#" <a href="/api/jobs/{}/preview-download" class="btn-secondary">Preview</a>"#,
            job.id
        )
    } else {
        String::new()
    };
    format!(
        r#"<tr id="job-{id}" hx-get="/api/jobs/{id}/status" hx-trigger="every 5s" hx-swap="outerHTML">
            <td>{title}</td>
            <td>{lang} {level}</td>
            <td>{status}</td>
            <td>{action}{preview_link}</td>
        </tr>"#,
        id = job.id,
        title = job.title.as_deref().unwrap_or("Untitled"),
        lang = job.target_lang,
        level = job.cefr_level,
        status = job.status,
        action = action,
        preview_link = preview_link,
    )
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let job = state
        .supabase
        .get_job(&job_id, &auth.user_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if job.status != "completed" {
        return Err(StatusCode::NOT_FOUND);
    }

    let path = job.output_file_path.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let bytes = state
        .supabase
        .download_file("uploads", path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let filename = format!(
        "{}-{}-{}.epub",
        job.title.as_deref().unwrap_or("book"),
        job.target_lang,
        job.cefr_level
    );

    Ok((
        [
            (header::CONTENT_TYPE, "application/epub+zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn download_preview(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let job = state
        .supabase
        .get_job(&job_id, &auth.user_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let path = job.preview_file_path.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let bytes = state
        .supabase
        .download_file("uploads", path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/epub+zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"preview.epub\"".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// Called by the job worker after payment is confirmed.
/// Semaphore and active_jobs tracking are managed by the caller (job_worker in main.rs).
pub async fn process_job(state: Arc<AppState>, job_id: String) {
    use gunnlod_core::pipeline::{run_pipeline, PipelineConfig};

    let _ = state
        .supabase
        .update_job(
            &job_id,
            &serde_json::json!({
                "status": "processing",
                "started_at": chrono::Utc::now(),
            }),
        )
        .await;

    let result = async {
        let jobs: Vec<crate::services::supabase::Job> = state
            .supabase
            .get(&format!("jobs?id=eq.{}&select=*", job_id))
            .await?;
        let job = jobs.into_iter().next().ok_or_else(|| anyhow::anyhow!("job not found"))?;

        let epub_bytes = state
            .supabase
            .download_file("uploads", job.source_file_path.as_deref().unwrap_or(""))
            .await?;

        let tmp_in = tempfile::NamedTempFile::new()?;
        std::fs::write(tmp_in.path(), &epub_bytes)?;

        let tmp_out = tempfile::Builder::new().suffix(".epub").tempfile()?;

        let config = PipelineConfig {
            source_lang: job.source_lang.clone(),
            target_lang: job.target_lang.clone(),
            level: job.cefr_level.clone(),
            chapters: vec![],
            max_chunk_words: 2500,
            force_two_pass: false,
        };

        run_pipeline(
            tmp_in.path(),
            tmp_out.path(),
            &config,
            state.translator.as_ref(),
            state.translator.as_ref(),
        )
        .await?;

        let out_bytes = std::fs::read(tmp_out.path())?;
        let out_path = format!("outputs/{}/{}.epub", job.user_id, job_id);
        state.supabase.upload_file("uploads", &out_path, out_bytes).await?;

        if let Some(src) = &job.source_file_path {
            let _ = state.supabase.delete_file("uploads", src).await;
        }

        Ok::<String, anyhow::Error>(out_path)
    }
    .await;

    match result {
        Ok(out_path) => {
            let _ = state
                .supabase
                .update_job(
                    &job_id,
                    &serde_json::json!({
                        "status": "completed",
                        "output_file_path": out_path,
                        "completed_at": chrono::Utc::now(),
                    }),
                )
                .await;
        }
        Err(e) => {
            let jobs: Vec<crate::services::supabase::Job> = state
                .supabase
                .get(&format!(
                    "jobs?id=eq.{}&select=retry_count,source_file_path",
                    job_id
                ))
                .await
                .unwrap_or_default();

            if let Some(job) = jobs.into_iter().next() {
                let new_retry = job.retry_count + 1;
                let is_final = new_retry >= 2;

                if is_final {
                    if let Some(src) = &job.source_file_path {
                        let _ = state.supabase.delete_file("uploads", src).await;
                    }
                    let _ = state
                        .supabase
                        .update_job(
                            &job_id,
                            &serde_json::json!({
                                "status": "failed",
                                "retry_count": new_retry,
                                "error_message": e.to_string(),
                            }),
                        )
                        .await;
                } else {
                    let _ = state
                        .supabase
                        .update_job(
                            &job_id,
                            &serde_json::json!({
                                "status": "paid",
                                "retry_count": new_retry,
                                "error_message": e.to_string(),
                            }),
                        )
                        .await;
                    // Re-enqueue after a delay via the job queue
                    let tx = state.job_tx.clone();
                    let job_id = job_id.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        let _ = tx.send(job_id);
                    });
                }
            }
        }
    }
}
