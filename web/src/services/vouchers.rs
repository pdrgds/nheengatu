use crate::services::supabase::{SupabaseClient, SupabaseError};
use uuid::Uuid;

pub struct VoucherInfo {
    pub id: Uuid,
    pub max_pages: i64,
}

impl SupabaseClient {
    /// Atomically redeem a voucher. Returns None if already used or not found.
    /// Uses UPDATE WHERE used_by IS NULL to prevent race conditions.
    pub async fn redeem_voucher(
        &self,
        code: &str,
        user_id: &str,
    ) -> Result<Option<VoucherInfo>, SupabaseError> {
        let resp: reqwest::Response = self
            .client
            .patch(format!("{}/rest/v1/vouchers", self.url))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "return=representation,count=exact")
            .query(&[
                ("code", format!("eq.{}", code)),
                ("used_by", "is.null".to_string()),
            ])
            .json(&serde_json::json!({
                "used_by": user_id,
                "used_at": chrono::Utc::now().to_rfc3339(),
            }))
            .send()
            .await?;

        // Content-Range: */0 means no rows matched (already used or doesn't exist)
        let matched = resp
            .headers()
            .get("Content-Range")
            .and_then(|v| v.to_str().ok())
            .map(|s| !s.ends_with("/0"))
            .unwrap_or(false);

        if !matched {
            return Ok(None);
        }

        let body: Vec<serde_json::Value> = resp.json().await?;
        let voucher = body.into_iter().next().ok_or(SupabaseError::NotFound)?;
        Ok(Some(VoucherInfo {
            id: voucher["id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or(SupabaseError::NotFound)?,
            max_pages: voucher["max_pages"].as_i64().unwrap_or(200),
        }))
    }
}
