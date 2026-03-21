use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::app_state::AppState;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    supabase_url: String,
    supabase_anon_key: String,
}

#[derive(Template)]
#[template(path = "signup.html")]
struct SignupTemplate {
    supabase_url: String,
    supabase_anon_key: String,
}

pub async fn login(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let tmpl = LoginTemplate {
        supabase_url: state.config.supabase_url.clone(),
        supabase_anon_key: state.config.supabase_anon_key.clone(),
    };
    Html(tmpl.render().unwrap_or_else(|e| format!("Template error: {e}")))
}

pub async fn signup(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let tmpl = SignupTemplate {
        supabase_url: state.config.supabase_url.clone(),
        supabase_anon_key: state.config.supabase_anon_key.clone(),
    };
    Html(tmpl.render().unwrap_or_else(|e| format!("Template error: {e}")))
}
