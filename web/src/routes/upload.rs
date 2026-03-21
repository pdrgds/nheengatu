use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Extension,
};
use std::sync::Arc;

use crate::{
    app_state::AppState,
    middleware::auth::AuthUser,
    services::pricing::calculate_price_cents,
};

pub async fn upload(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, StatusCode> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut target_lang = String::new();
    let mut cefr_level = String::new();
    let mut source_lang: Option<String> = None;
    let mut voucher_code: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        match field.name().unwrap_or("") {
            "file" => {
                let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                if bytes.len() > 50 * 1024 * 1024 {
                    return Err(StatusCode::PAYLOAD_TOO_LARGE);
                }
                // Validate magic bytes: EPUB is a ZIP file starting with PK\x03\x04
                if bytes.len() < 4 || &bytes[..4] != b"PK\x03\x04" {
                    return Err(StatusCode::UNPROCESSABLE_ENTITY);
                }
                file_bytes = Some(bytes.to_vec());
            }
            "target_lang" => target_lang = field.text().await.unwrap_or_default(),
            "cefr_level" => cefr_level = field.text().await.unwrap_or_default(),
            "source_lang" => {
                let v = field.text().await.unwrap_or_default();
                if !v.is_empty() {
                    source_lang = Some(v);
                }
            }
            "voucher" => {
                let v = field.text().await.unwrap_or_default();
                if !v.is_empty() {
                    voucher_code = Some(v);
                }
            }
            _ => {}
        }
    }

    let bytes = file_bytes.ok_or(StatusCode::BAD_REQUEST)?;
    if target_lang.is_empty() || cefr_level.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Parse EPUB to get metadata
    let tmp = tempfile::NamedTempFile::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(tmp.path(), &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let book = gunnlod_core::epub_parser::parse_epub(tmp.path())
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;

    let source_detected = source_lang.or(book.metadata.source_language.clone());
    let word_count = book.metadata.word_count;
    let price_cents = calculate_price_cents(word_count);
    let job_id = uuid::Uuid::new_v4().to_string();
    let source_path = format!("uploads/{}/{}.epub", auth.user_id, job_id);

    state
        .supabase
        .upload_file("uploads", &source_path, bytes)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let job = state
        .supabase
        .create_job(&serde_json::json!({
            "id": job_id,
            "user_id": auth.user_id,
            "status": "uploaded",
            "source_file_path": source_path,
            "source_lang": source_detected,
            "target_lang": target_lang,
            "cefr_level": cefr_level,
            "word_count": word_count,
            "chapter_count": book.metadata.chapter_count,
            "title": book.metadata.title,
            "price_cents": price_cents,
            "voucher_code": voucher_code,
            "pipeline": "single",
        }))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Spawn preview (first chapter, no payment required)
    {
        let state = state.clone();
        let job_id = job.id.to_string();
        tokio::spawn(async move {
            process_preview(state, job_id).await;
        });
    }

    Ok(Redirect::to(&format!("/dashboard?job={}", job.id)))
}

async fn process_preview(state: Arc<AppState>, job_id: String) {
    use gunnlod_core::pipeline::{run_pipeline, PipelineConfig};

    let _ = state.supabase.update_job_status(&job_id, "preview_processing").await;

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
            chapters: vec![1], // preview = first chapter only
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
        let preview_path = format!("previews/{}/{}-preview.epub", job.user_id, job_id);
        state.supabase.upload_file("uploads", &preview_path, out_bytes).await?;

        Ok::<String, anyhow::Error>(preview_path)
    }
    .await;

    match result {
        Ok(preview_path) => {
            let _ = state
                .supabase
                .update_job(
                    &job_id,
                    &serde_json::json!({
                        "status": "preview_ready",
                        "preview_file_path": preview_path,
                    }),
                )
                .await;
        }
        Err(e) => {
            tracing::warn!("Preview processing failed for job {}: {}", job_id, e);
            let _ = state.supabase.update_job_status(&job_id, "preview_failed").await;
        }
    }
}
