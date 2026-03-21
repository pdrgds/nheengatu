use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TranslateError {
    #[error("API request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("missing API key")]
    MissingApiKey,
}

#[async_trait]
pub trait Translator: Send + Sync {
    async fn translate_chunk(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
    ) -> Result<String, TranslateError>;
}

pub struct GroqTranslator {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GroqTranslator {
    pub fn new(api_key: String) -> Result<Self, TranslateError> {
        if api_key.is_empty() {
            return Err(TranslateError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            model: "llama-3.3-70b-versatile".to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        })
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn build_prompt(
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
        prev_context: Option<&str>,
    ) -> String {
        let context_block = prev_context
            .map(|ctx| {
                format!(
                    "Context from the previous section (for vocabulary consistency \
                     — do not translate this):\n{}\n\n---\n\n",
                    ctx
                )
            })
            .unwrap_or_default();

        format!(
            "{}Translate the following text from {} to {}.\n\
             Simplify it to CEFR level {}. Use only grammar and vocabulary appropriate for {}.\n\
             Keep the meaning and tone of the original. Do not add explanations or notes.\n\
             Output only the translated and simplified text.\n\nText:\n{}",
            context_block, source_lang, target_lang, level, level, text
        )
    }
}

#[async_trait]
impl Translator for GroqTranslator {
    async fn translate_chunk(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = Self::build_prompt(text, source_lang, target_lang, level, None);

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.3,
                "max_tokens": 8192,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslateError::ApiError(format!("{}: {}", status, body)));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }
}

pub struct OllamaTranslator {
    pub base_url: String,
    pub model: String,
    client: reqwest::Client,
}

impl OllamaTranslator {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".into()),
            model: model.unwrap_or_else(|| "llama3.1:8b".into()),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(300)) // local models can be slow
                .connect_timeout(Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl Translator for OllamaTranslator {
    async fn translate_chunk(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = GroqTranslator::build_prompt(text, source_lang, target_lang, level, None);

        let resp = self
            .client
            .post(format!("{}/api/generate", self.base_url))
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": prompt,
                "stream": false,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(TranslateError::ApiError(
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["response"].as_str().unwrap_or("").to_string())
    }
}

/// Translate all chunks sequentially, passing last-200-words context to each chunk
/// for vocabulary consistency. Retries up to 3 times per chunk with backoff.
/// On rate-limit errors (429), waits 30s before retrying.
pub async fn translate_chunks(
    translator: &dyn Translator,
    chunks: &[crate::book::Chunk],
    source_lang: &str,
    target_lang: &str,
    level: &str,
) -> Result<Vec<String>, TranslateError> {
    let mut results = Vec::new();
    let mut prev_context: Option<String> = None;

    for chunk in chunks.iter() {
        // Prepend last-200-words context as plain text so the translator sees it
        // in the "Text:" section of the prompt. Do NOT pre-assemble a full prompt
        // here — translate_chunk does that internally.
        let text = match &prev_context {
            Some(ctx) => format!(
                "[Vocabulary reference from previous section — do not translate this part]\n\
                 {}\n\n---\n\n{}",
                ctx, chunk.content
            ),
            None => chunk.content.clone(),
        };

        let mut last_err = None;
        let mut translated = String::new();

        for attempt in 0..3u32 {
            match translator
                .translate_chunk(&text, source_lang, target_lang, level)
                .await
            {
                Ok(t) => {
                    translated = t;
                    last_err = None;
                    break;
                }
                Err(e) => {
                    // Use longer backoff for rate limit errors
                    let is_rate_limit = format!("{}", e).contains("429");
                    let wait_secs = if is_rate_limit { 30u64 } else { 2u64.pow(attempt) };
                    last_err = Some(e);
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                    }
                }
            }
        }

        if let Some(e) = last_err {
            return Err(e);
        }

        // Extract last ~200 words as context for the next chunk
        let words: Vec<&str> = translated.split_whitespace().collect();
        let ctx_start = words.len().saturating_sub(200);
        prev_context = Some(words[ctx_start..].join(" "));

        results.push(translated);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_key() {
        assert!(matches!(
            GroqTranslator::new(String::new()),
            Err(TranslateError::MissingApiKey)
        ));
    }

    #[test]
    fn prompt_includes_all_params() {
        let p = GroqTranslator::build_prompt("Hello world.", "en", "de", "A2", None);
        assert!(
            p.contains("en") && p.contains("de") && p.contains("A2") && p.contains("Hello world.")
        );
    }

    #[test]
    fn prompt_includes_context_when_provided() {
        let ctx = "some previous context";
        let p = GroqTranslator::build_prompt("New text.", "en", "de", "A2", Some(ctx));
        assert!(p.contains(ctx));
        assert!(p.contains("New text."));
    }

    #[test]
    fn ollama_defaults() {
        let t = OllamaTranslator::new(None, None);
        assert_eq!(t.base_url, "http://localhost:11434");
        assert_eq!(t.model, "llama3.1:8b");
    }
}
