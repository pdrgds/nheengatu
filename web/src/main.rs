mod app_state;
mod config;
mod middleware;
mod routes;
mod services;

use app_state::AppState;
use axum::{
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};
use config::Config;
use gunnlod_core::translator::GroqTranslator;
use services::supabase::SupabaseClient;
use std::sync::{atomic::AtomicUsize, Arc};
use tokio::sync::{mpsc, Semaphore};
use tower_http::limit::RequestBodyLimitLayer;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let config = Config::from_env();

    let translator: Arc<dyn gunnlod_core::translator::Translator + Send + Sync> = Arc::new(
        GroqTranslator::new(config.groq_api_key.clone()).expect("Invalid GROQ_API_KEY"),
    );
    let supabase = Arc::new(SupabaseClient::new(
        &config.supabase_url,
        &config.supabase_service_role_key,
    ));
    let job_semaphore = Arc::new(Semaphore::new(5));
    let active_jobs = Arc::new(AtomicUsize::new(0));

    let (job_tx, job_rx) = mpsc::unbounded_channel::<String>();

    let addr = format!("0.0.0.0:{}", config.port);
    let state = Arc::new(AppState {
        config,
        translator,
        supabase,
        job_semaphore,
        active_jobs,
        job_tx,
    });

    // Recover stuck jobs from a previous crash/restart
    recover_stuck_jobs(&state).await;

    // Background job queue worker: reads job IDs from the channel, enforces concurrency
    tokio::spawn({
        let state = state.clone();
        async move { job_worker(state, job_rx).await }
    });

    // Keepalive ping to prevent Supabase free-tier pausing after 7 days of inactivity
    tokio::spawn({
        let supabase = state.supabase.clone();
        async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(3 * 24 * 3600));
            interval.tick().await; // skip first tick
            loop {
                interval.tick().await;
                let _ = supabase.health_ping().await;
                tracing::debug!("Supabase keepalive ping sent");
            }
        }
    });

    let protected = Router::new()
        .route("/upload", get(routes::pages::upload_page))
        .route(
            "/api/upload",
            post(routes::upload::upload)
                .layer(RequestBodyLimitLayer::new(50 * 1024 * 1024)),
        )
        .route("/dashboard", get(routes::jobs::dashboard))
        .route("/api/jobs/:id/status", get(routes::jobs::job_status))
        .route("/api/jobs/:id/download", get(routes::jobs::download))
        .route(
            "/api/jobs/:id/preview-download",
            get(routes::jobs::download_preview),
        )
        .route("/api/jobs/:id/pay", get(routes::payment::pay))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::require_auth,
        ));

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route("/", get(routes::pages::landing))
        .route("/login", get(routes::auth::login))
        .route("/signup", get(routes::auth::signup))
        .route("/api/stripe/webhook", post(routes::payment::stripe_webhook))
        .merge(protected)
        .with_state(state);

    tracing::info!("Starting on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Reads job IDs from the channel and processes them with concurrency capped by job_semaphore.
async fn job_worker(state: Arc<AppState>, mut job_rx: mpsc::UnboundedReceiver<String>) {
    while let Some(job_id) = job_rx.recv().await {
        let permit = Arc::clone(&state.job_semaphore)
            .acquire_owned()
            .await
            .unwrap();
        state
            .active_jobs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let state = state.clone();
        tokio::spawn(async move {
            let _permit = permit;
            routes::jobs::process_job(state.clone(), job_id).await;
            state
                .active_jobs
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });
    }
}

async fn recover_stuck_jobs(state: &Arc<AppState>) {
    match state.supabase.get_stuck_jobs().await {
        Ok(jobs) if !jobs.is_empty() => {
            tracing::warn!("Recovering {} stuck jobs from previous run", jobs.len());
            for job in jobs {
                let _ = state
                    .supabase
                    .update_job_status(&job.id.to_string(), "paid")
                    .await;
                let _ = state.job_tx.send(job.id.to_string());
            }
        }
        Ok(_) => {}
        Err(e) => tracing::error!("Failed to check for stuck jobs on startup: {}", e),
    }
}
