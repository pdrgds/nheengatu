use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::app_state::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

/// Injected into request extensions for all authenticated routes.
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub user_id: String,
}

pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = extract_sb_token(&req).ok_or(StatusCode::UNAUTHORIZED)?;

    let key = DecodingKey::from_secret(state.config.supabase_jwt_secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["authenticated"]);

    let claims = decode::<Claims>(&token, &key, &validation)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .claims;

    req.extensions_mut().insert(AuthUser { user_id: claims.sub });
    Ok(next.run(req).await)
}

fn extract_sb_token(req: &Request) -> Option<String> {
    req.headers()
        .get("Cookie")?
        .to_str()
        .ok()?
        .split(';')
        .map(|c| c.trim())
        .find(|c| c.starts_with("sb-token="))?
        .strip_prefix("sb-token=")
        .map(|s| s.to_string())
}
