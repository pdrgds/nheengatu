use askama::Template;
use axum::response::{Html, IntoResponse};

#[derive(Template)]
#[template(path = "landing.html")]
pub struct LandingTemplate;

pub async fn landing() -> impl IntoResponse {
    Html(LandingTemplate.render().unwrap_or_else(|e| format!("Template error: {e}")))
}

#[derive(Template)]
#[template(path = "upload.html")]
pub struct UploadTemplate;

pub async fn upload_page() -> impl IntoResponse {
    Html(UploadTemplate.render().unwrap_or_else(|e| format!("Template error: {e}")))
}
