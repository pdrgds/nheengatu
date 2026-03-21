use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum SupabaseError {
    #[error("request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("API error {status}: {body}")]
    ApiError { status: u16, body: String },
    #[error("not found")]
    NotFound,
}


pub struct SupabaseClient {
    pub url: String,
    pub(crate) service_role_key: String,
    pub(crate) client: Client,
}

impl SupabaseClient {
    pub fn new(url: &str, service_role_key: &str) -> Self {
        Self {
            url: url.to_string(),
            service_role_key: service_role_key.to_string(),
            client: Client::new(),
        }
    }

    pub async fn health_ping(&self) -> Result<(), SupabaseError> {
        let resp = self
            .client
            .get(format!("{}/rest/v1/", self.url))
            .header("apikey", &self.service_role_key)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SupabaseError::ApiError { status, body });
        }
        Ok(())
    }

    pub(crate) async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, SupabaseError> {
        let resp = self
            .client
            .get(format!("{}/rest/v1/{}", self.url, path))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Accept", "application/json")
            .send()
            .await?;
        if resp.status() == 404 {
            return Err(SupabaseError::NotFound);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SupabaseError::ApiError { status, body });
        }
        Ok(resp.json().await?)
    }

    async fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, SupabaseError> {
        let resp = self
            .client
            .post(format!("{}/rest/v1/{}", self.url, path))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "return=representation")
            .json(body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SupabaseError::ApiError { status, body });
        }
        Ok(resp
            .json::<Vec<T>>()
            .await?
            .into_iter()
            .next()
            .ok_or(SupabaseError::NotFound)?)
    }

    /// All job queries MUST include user_id to prevent cross-user access
    /// (service role bypasses RLS).
    pub async fn get_job(&self, job_id: &str, user_id: &str) -> Result<Job, SupabaseError> {
        self.get(&format!("jobs?id=eq.{}&user_id=eq.{}&select=*", job_id, user_id))
            .await
            .and_then(|v: Vec<Job>| v.into_iter().next().ok_or(SupabaseError::NotFound))
    }

    pub async fn get_jobs_for_user(&self, user_id: &str) -> Result<Vec<Job>, SupabaseError> {
        self.get(&format!("jobs?user_id=eq.{}&select=*&order=created_at.desc", user_id))
            .await
    }

    pub async fn create_job(&self, job: &serde_json::Value) -> Result<Job, SupabaseError> {
        self.post_json("jobs", job).await
    }

    pub async fn update_job_status(&self, job_id: &str, status: &str) -> Result<(), SupabaseError> {
        self.client
            .patch(format!("{}/rest/v1/jobs?id=eq.{}", self.url, job_id))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .json(&serde_json::json!({ "status": status }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn update_job(&self, job_id: &str, updates: &serde_json::Value) -> Result<(), SupabaseError> {
        self.client
            .patch(format!("{}/rest/v1/jobs?id=eq.{}", self.url, job_id))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .json(updates)
            .send()
            .await?;
        Ok(())
    }

    /// Used by Stripe webhook idempotency check.
    pub async fn get_job_by_stripe_payment(&self, payment_id: &str) -> Result<Option<Job>, SupabaseError> {
        let jobs: Vec<Job> = self
            .get(&format!("jobs?stripe_payment_id=eq.{}&select=*", payment_id))
            .await?;
        Ok(jobs.into_iter().next())
    }

    /// Used by startup recovery: find jobs stuck in processing states.
    pub async fn get_stuck_jobs(&self) -> Result<Vec<Job>, SupabaseError> {
        self.get(
            "jobs?status=in.(processing,preview_processing)&updated_at=lt.NOW()-interval '15 minutes'&select=*",
        )
        .await
    }

    pub async fn upload_file(&self, bucket: &str, path: &str, data: Vec<u8>) -> Result<(), SupabaseError> {
        let resp = self
            .client
            .post(format!("{}/storage/v1/object/{}/{}", self.url, bucket, path))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Content-Type", "application/epub+zip")
            .body(data)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SupabaseError::ApiError { status, body });
        }
        Ok(())
    }

    pub async fn download_file(&self, bucket: &str, path: &str) -> Result<Vec<u8>, SupabaseError> {
        let resp = self
            .client
            .get(format!("{}/storage/v1/object/{}/{}", self.url, bucket, path))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .send()
            .await?;
        if resp.status() == 404 {
            return Err(SupabaseError::NotFound);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SupabaseError::ApiError { status, body });
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn delete_file(&self, bucket: &str, path: &str) -> Result<(), SupabaseError> {
        self.client
            .delete(format!("{}/storage/v1/object/{}/{}", self.url, bucket, path))
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .send()
            .await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Job {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub source_file_path: Option<String>,
    pub output_file_path: Option<String>,
    pub preview_file_path: Option<String>,
    pub source_lang: Option<String>,
    pub target_lang: String,
    pub cefr_level: String,
    pub word_count: Option<i64>,
    pub chapter_count: Option<i64>,
    pub title: Option<String>,
    pub price_cents: Option<i64>,
    pub stripe_payment_id: Option<String>,
    pub voucher_id: Option<Uuid>,
    pub retry_count: i64,
    pub pipeline: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error_message: Option<String>,
}
