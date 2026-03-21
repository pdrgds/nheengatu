use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Extension,
};
use std::{collections::HashMap, sync::Arc};

use crate::{app_state::AppState, middleware::auth::AuthUser};

pub async fn pay(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(job_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StatusCode> {
    let job = state
        .supabase
        .get_job(&job_id, &auth.user_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if let Some(code) = params.get("voucher").filter(|v| !v.is_empty()) {
        match state.supabase.redeem_voucher(code, &auth.user_id).await {
            Ok(Some(voucher)) => {
                state
                    .supabase
                    .update_job(
                        &job_id,
                        &serde_json::json!({
                            "status": "paid",
                            "voucher_id": voucher.id,
                        }),
                    )
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                let _ = state.job_tx.send(job_id);
                return Ok(Redirect::to("/dashboard").into_response());
            }
            Ok(None) => {
                tracing::warn!("Voucher '{}' not valid for user {}", code, auth.user_id);
            }
            Err(e) => {
                tracing::error!("Voucher check failed: {}", e);
            }
        }
    }

    // Create Stripe Checkout session
    let price_cents = job.price_cents.unwrap_or(500);
    let success_url = format!("{}/dashboard", state.config.app_base_url);
    let cancel_url = format!("{}/dashboard", state.config.app_base_url);
    let product_name = job.title.as_deref().unwrap_or("Book Translation").to_string();

    let resp = reqwest::Client::new()
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&state.config.stripe_secret_key, Option::<&str>::None)
        .form(&[
            ("mode", "payment"),
            ("line_items[0][price_data][currency]", "eur"),
            (
                "line_items[0][price_data][unit_amount]",
                &price_cents.to_string(),
            ),
            (
                "line_items[0][price_data][product_data][name]",
                &product_name,
            ),
            ("line_items[0][quantity]", "1"),
            ("metadata[job_id]", &job_id),
            ("success_url", &success_url),
            ("cancel_url", &cancel_url),
        ])
        .send()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let session: serde_json::Value =
        resp.json().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let checkout_url = session["url"].as_str().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(checkout_url).into_response())
}

/// Stripe webhook: mark job paid and start processing.
/// Idempotent: silently succeeds if job is already paid/processing/completed.
pub async fn stripe_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, StatusCode> {
    let sig = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    verify_stripe_signature(&body, sig, &state.config.stripe_webhook_secret)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let event: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    if event["type"].as_str() != Some("checkout.session.completed") {
        return Ok(StatusCode::OK);
    }

    let payment_id = event["data"]["object"]["id"]
        .as_str()
        .ok_or(StatusCode::BAD_REQUEST)?;
    let job_id = event["data"]["object"]["metadata"]["job_id"]
        .as_str()
        .ok_or(StatusCode::BAD_REQUEST)?
        .to_string();

    // Idempotency check
    if let Ok(Some(existing)) = state.supabase.get_job_by_stripe_payment(payment_id).await {
        if matches!(
            existing.status.as_str(),
            "paid" | "processing" | "completed"
        ) {
            tracing::info!("Webhook already processed for payment {}", payment_id);
            return Ok(StatusCode::OK);
        }
    }

    state
        .supabase
        .update_job(
            &job_id,
            &serde_json::json!({
                "status": "paid",
                "stripe_payment_id": payment_id,
            }),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = state.job_tx.send(job_id);

    Ok(StatusCode::OK)
}

fn verify_stripe_signature(payload: &[u8], sig_header: &str, secret: &str) -> Result<(), ()> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut timestamp = "";
    let mut expected_sig = "";
    for part in sig_header.split(',') {
        if let Some(t) = part.strip_prefix("t=") {
            timestamp = t;
        }
        if let Some(v) = part.strip_prefix("v1=") {
            expected_sig = v;
        }
    }
    if timestamp.is_empty() || expected_sig.is_empty() {
        return Err(());
    }

    let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| ())?;
    mac.update(signed_payload.as_bytes());
    let result = mac.finalize().into_bytes();
    let computed = hex::encode(result);

    if computed == expected_sig { Ok(()) } else { Err(()) }
}
