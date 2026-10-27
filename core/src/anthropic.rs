use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;

use crate::translator::{build_prompt, build_simplify_prompt, TranslateError, Translator};

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
const ANTHROPIC_VERSION: &str = "2026-10-21";
const SERVER_SIDE_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const REQUEST_TIMEOUT_SECS: u64 = 300;
const MAX_TOKENS: u32 = 16000;

/// Token and fallback counts accumulated across every request a translator
/// has made, so a run can report what it cost.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub fallback_responses: u64,
}

/// Calls Claude through the Anthropic Messages API.
///
/// API version 2026-10-21 rejects `temperature`, so unlike the OpenRouter
/// client no sampling parameters are sent. Depth of thinking is set through
/// `effort` instead; translation needs little of it, so the default is `low`.
pub struct AnthropicTranslator {
    api_key: String,
    model: String,
    base_url: String,
    effort: String,
    client: reqwest::Client,
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    fallback_responses: AtomicU64,
    pub simple_prompt: bool,
}

impl AnthropicTranslator {
    pub fn new(api_key: String) -> Result<Self, TranslateError> {
        if api_key.is_empty() {
            return Err(TranslateError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            model: DEFAULT_MODEL.to_string(),
            base_url: ANTHROPIC_API_URL.to_string(),
            effort: "low".to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            fallback_responses: AtomicU64::new(0),
            simple_prompt: false,
        })
    }

    /// `url` is the complete Messages endpoint — tests inject a mock server here.
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    pub fn with_effort(mut self, effort: String) -> Self {
        self.effort = effort;
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn usage(&self) -> Usage {
        Usage {
            input_tokens: self.input_tokens.load(Ordering::Relaxed),
            output_tokens: self.output_tokens.load(Ordering::Relaxed),
            fallback_responses: self.fallback_responses.load(Ordering::Relaxed),
        }
    }

    /// Models whose declined requests the API can re-run on another model.
    /// Claude Haiku 5.5 has no server-side fallback, and sending the field to
    /// it is pointless at best.
    fn supports_server_side_fallback(model: &str) -> bool {
        matches!(
            model,
            "claude-fable-5-1" | "claude-opus-5-5" | "claude-opus-5" | "claude-sonnet-5-5"
        )
    }

    fn request_body(&self, prompt: &str) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": [{"role": "user", "content": prompt}],
            "output_config": {"effort": self.effort},
        });
        if Self::supports_server_side_fallback(&self.model) {
            body["fallbacks"] = serde_json::Value::from("default");
        }
        body
    }

    fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
        headers
            .get(reqwest::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    }

    /// Send one request and return the concatenated text blocks.
    ///
    /// A refusal arrives as HTTP 200 with `stop_reason: "refusal"`; it is
    /// returned as an error so the caller retries instead of writing the
    /// decline into the book. A reply cut off at `max_tokens` is an error for
    /// the same reason: a truncated chunk silently drops part of the text.
    async fn complete(&self, prompt: &str) -> Result<String, TranslateError> {
        let mut request = self
            .client
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&self.request_body(prompt));
        if Self::supports_server_side_fallback(&self.model) {
            request = request.header("anthropic-beta", SERVER_SIDE_FALLBACK_BETA);
        }
        let resp = request.send().await?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = Self::parse_retry_after(resp.headers());
            return Err(TranslateError::RateLimited { retry_after });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslateError::ApiError(format!("{}: {}", status.as_u16(), body)));
        }

        let body: serde_json::Value = resp.json().await?;
        if let Some(usage) = body.get("usage") {
            let read = |field: &str| usage.get(field).and_then(|v| v.as_u64()).unwrap_or(0);
            self.input_tokens.fetch_add(read("input_tokens"), Ordering::Relaxed);
            self.output_tokens.fetch_add(read("output_tokens"), Ordering::Relaxed);
        }
        if body["model"].as_str().is_some_and(|served| served != self.model) {
            self.fallback_responses.fetch_add(1, Ordering::Relaxed);
        }

        match body["stop_reason"].as_str() {
            Some("refusal") => {
                let category = body["stop_details"]["category"].as_str().unwrap_or("unspecified");
                return Err(TranslateError::Refused(category.to_string()));
            }
            Some("max_tokens") => {
                return Err(TranslateError::ApiError(format!(
                    "response cut off at max_tokens ({MAX_TOKENS})"
                )));
            }
            _ => {}
        }

        let text: String = body["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(TranslateError::ApiError("no text content in API response".to_string()));
        }
        Ok(text)
    }
}

#[async_trait]
impl Translator for AnthropicTranslator {
    async fn translate_chunk(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = build_prompt(text, source_lang, target_lang, level, None, self.simple_prompt);
        self.complete(&prompt).await
    }

    async fn simplify_chunk(
        &self,
        text: &str,
        lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = build_simplify_prompt(text, lang, level, self.simple_prompt);
        self.complete(&prompt).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn translator_for(server: &MockServer, model: &str) -> AnthropicTranslator {
        AnthropicTranslator::new("test-key".to_string())
            .unwrap()
            .with_model(model.to_string())
            .with_base_url(format!("{}/v1/messages", server.uri()))
    }

    fn message(model: &str, stop_reason: &str, content: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "model": model,
            "stop_reason": stop_reason,
            "content": content,
            "usage": {"input_tokens": 100, "output_tokens": 40},
        })
    }

    #[tokio::test]
    async fn returns_text_blocks_and_skips_thinking() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2026-10-21"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message(
                "claude-haiku-5-5",
                "end_turn",
                serde_json::json!([
                    {"type": "thinking", "thinking": "", "signature": "sig"},
                    {"type": "text", "text": "Olá "},
                    {"type": "text", "text": "mundo."}
                ]),
            )))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-haiku-5-5");
        let out = t.translate_chunk("Hello world.", "en", "pt", "A2").await.unwrap();
        assert_eq!(out, "Olá mundo.");
        assert_eq!(t.usage(), Usage { input_tokens: 100, output_tokens: 40, fallback_responses: 0 });
    }

    #[tokio::test]
    async fn sends_effort_and_no_sampling_parameters() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_partial_json(serde_json::json!({
                "model": "claude-haiku-5-5",
                "output_config": {"effort": "low"},
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(message(
                "claude-haiku-5-5",
                "end_turn",
                serde_json::json!([{"type": "text", "text": "ok"}]),
            )))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-haiku-5-5");
        t.simplify_chunk("Hello.", "en", "A2").await.unwrap();

        let requests: Vec<Request> = server.received_requests().await.unwrap();
        let body: serde_json::Value = requests[0].body_json().unwrap();
        for field in ["temperature", "top_p", "top_k", "fallbacks"] {
            assert!(body.get(field).is_none(), "{field} must not be sent to Haiku: {body}");
        }
        assert!(requests[0].headers.get("anthropic-beta").is_none());
    }

    #[tokio::test]
    async fn opts_sonnet_into_server_side_fallback() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("anthropic-beta", "server-side-fallback-2026-07-01"))
            .and(body_partial_json(serde_json::json!({"fallbacks": "default"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(message(
                "claude-sonnet-5",
                "end_turn",
                serde_json::json!([{"type": "text", "text": "ok"}]),
            )))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-sonnet-5-5");
        t.translate_chunk("Hello.", "en", "pt", "A2").await.unwrap();
        assert_eq!(t.usage().fallback_responses, 1, "served by another model, so it counts");
    }

    #[tokio::test]
    async fn refusal_is_an_error_not_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "claude-haiku-5-5",
                "stop_reason": "refusal",
                "stop_details": {"type": "refusal", "category": "general_harms"},
                "content": [],
                "usage": {"input_tokens": 100, "output_tokens": 0},
            })))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-haiku-5-5");
        let err = t.simplify_chunk("Hello.", "en", "A2").await.unwrap_err();
        match err {
            TranslateError::Refused(category) => assert_eq!(category, "general_harms"),
            other => panic!("expected Refused, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn truncated_reply_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message(
                "claude-haiku-5-5",
                "max_tokens",
                serde_json::json!([{"type": "text", "text": "half a chap"}]),
            )))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-haiku-5-5");
        let err = t.translate_chunk("Hello.", "en", "pt", "A2").await.unwrap_err();
        assert!(matches!(err, TranslateError::ApiError(m) if m.contains("max_tokens")));
    }

    #[tokio::test]
    async fn maps_429_to_rate_limited_with_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "20"))
            .mount(&server)
            .await;

        let t = translator_for(&server, "claude-opus-5-5");
        let err = t.translate_chunk("Hello.", "en", "pt", "A2").await.unwrap_err();
        assert!(matches!(err, TranslateError::RateLimited { retry_after: Some(20) }));
    }

    #[test]
    fn defaults_to_haiku_5_5() {
        let t = AnthropicTranslator::new("key".to_string()).unwrap();
        assert_eq!(t.model(), "claude-haiku-5-5");
        assert_eq!(t.base_url, ANTHROPIC_API_URL);
    }

    #[test]
    fn rejects_empty_key() {
        assert!(matches!(
            AnthropicTranslator::new(String::new()),
            Err(TranslateError::MissingApiKey)
        ));
    }
}
