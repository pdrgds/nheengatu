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

    async fn simplify_chunk(
        &self,
        text: &str,
        lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        // Default: fall back to translate_chunk with same source/target
        self.translate_chunk(text, lang, lang, level).await
    }
}

pub struct GroqTranslator {
    api_key: String,
    model: String,
    client: reqwest::Client,
    pub simple_prompt: bool,
}

impl GroqTranslator {
    pub fn new(api_key: String) -> Result<Self, TranslateError> {
        if api_key.is_empty() {
            return Err(TranslateError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            model: "llama-3.3-70b-versatile".to_string(),
            simple_prompt: false,
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
        simple_prompt: bool,
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

        if simple_prompt {
            return format!(
                "{}Translate the following text from {} to {}. \
                 Simplify it to CEFR level {}.\n\
                 Output ONLY the translated text, nothing else.\n\nText:\n{}",
                context_block, source_lang, target_lang, level, text
            );
        }

        let level_instructions = match level {
            "A1" => concat!(
                "You are writing for a complete beginner. Simplify the story aggressively.\n",
                "Rules:\n",
                "- One idea per sentence. Maximum 8 words per sentence.\n",
                "- Only present tense and simple past (he went, she said).\n",
                "- No subordinate clauses, no relative clauses, no complex grammar.\n",
                "- Replace every difficult word with the simplest possible word.\n",
                "- If a scene is complex, reduce it to the main action only.\n",
                "- It is OK to lose detail. Clarity is more important than completeness.\n",
                "\n",
                "Style example (apply this to the target language):\n",
                "WRONG: \"Despite his miserable living conditions, Harry maintained a resilient spirit.\"\n",
                "RIGHT: \"Harry was sad. But he did not give up.\"\n",
                "\n",
                "WRONG: \"The letter, which had arrived mysteriously, contained an extraordinary invitation.\"\n",
                "RIGHT: \"A letter came. It had an invitation. Harry was surprised.\"\n",
            ),
            "A2" => concat!(
                "You are writing for an elementary learner. Simplify language and sentence structure.\n",
                "Rules:\n",
                "- Short, clear sentences (under 15 words). Two ideas per sentence at most.\n",
                "- Use everyday vocabulary. Replace rare or formal words with common ones.\n",
                "- Simple past and present tense. Basic connectors only: and, but, because, so, then.\n",
                "- Keep the full story but simplify how it is told. You can drop small details.\n",
                "\n",
                "Style example (apply this to the target language):\n",
                "WRONG: \"He resided with his relatives, who harboured a profound aversion towards him.\"\n",
                "RIGHT: \"He lived with his aunt and uncle. They did not like him very much.\"\n",
                "\n",
                "WRONG: \"The magnificent owl descended gracefully, bearing a sealed parchment.\"\n",
                "RIGHT: \"An owl flew down. It had a letter.\"\n",
            ),
            "B1" => concat!(
                "You are writing for an intermediate learner.\n",
                "Rules:\n",
                "- Use clear, natural language. Sentences can be moderate length.\n",
                "- Avoid rare words, idioms, and overly complex structures.\n",
                "- Keep the full story and most details. Simplify only where needed.\n",
            ),
            "B2" => concat!(
                "You are writing for an upper-intermediate learner.\n",
                "Rules:\n",
                "- Preserve the original style and narrative closely.\n",
                "- Simplify only unusually complex sentences and rare vocabulary.\n",
                "- Replace C1/C2 words with B2-level equivalents where possible.\n",
            ),
            _ => "Translate faithfully, preserving the original style and vocabulary.\n",
        };

        format!(
            "{}Translate the following text from {} to {}.\n\
             Target level: CEFR {}.\n\n\
             {}\n\
             Do NOT include any introduction, explanation, or commentary.\n\
             Do NOT write sentences like \"Here is the translation\" or \"Here is the simplified text\".\n\
             Output ONLY the translated and simplified text, nothing else.\n\nText:\n{}",
            context_block, source_lang, target_lang, level, level_instructions, text
        )
    }

    pub fn build_simplify_prompt(
        text: &str,
        lang: &str,
        level: &str,
        simple_prompt: bool,
    ) -> String {
        if simple_prompt {
            return format!(
                "Rewrite the following text in {lang}. \
                 Simplify it to CEFR level {level}. Keep it in {lang}. Do not translate it.\n\
                 Output ONLY the rewritten text, nothing else.\n\nText:\n{text}"
            );
        }

        let level_instructions = match level {
            "A1" => concat!(
                "You are writing for a complete beginner. Simplify the story aggressively.\n",
                "Rules:\n",
                "- One idea per sentence. Maximum 8 words per sentence.\n",
                "- Only present tense and simple past (he went, she said).\n",
                "- No subordinate clauses, no relative clauses, no complex grammar.\n",
                "- Replace every difficult word with the simplest possible word.\n",
                "- If a scene is complex, reduce it to the main action only.\n",
                "- It is OK to lose detail. Clarity is more important than completeness.\n",
                "\n",
                "Style example (apply this to the target language):\n",
                "WRONG: \"Despite his miserable living conditions, Harry maintained a resilient spirit.\"\n",
                "RIGHT: \"Harry was sad. But he did not give up.\"\n",
                "\n",
                "WRONG: \"The letter, which had arrived mysteriously, contained an extraordinary invitation.\"\n",
                "RIGHT: \"A letter came. It had an invitation. Harry was surprised.\"\n",
            ),
            "A2" => concat!(
                "You are writing for an elementary learner. Simplify language and sentence structure.\n",
                "Rules:\n",
                "- Short, clear sentences (under 15 words). Two ideas per sentence at most.\n",
                "- Use everyday vocabulary. Replace rare or formal words with common ones.\n",
                "- Simple past and present tense. Basic connectors only: and, but, because, so, then.\n",
                "- Keep the full story but simplify how it is told. You can drop small details.\n",
                "\n",
                "Style example (apply this to the target language):\n",
                "WRONG: \"He resided with his relatives, who harboured a profound aversion towards him.\"\n",
                "RIGHT: \"He lived with his aunt and uncle. They did not like him very much.\"\n",
                "\n",
                "WRONG: \"The magnificent owl descended gracefully, bearing a sealed parchment.\"\n",
                "RIGHT: \"An owl flew down. It had a letter.\"\n",
            ),
            "B1" => concat!(
                "You are writing for an intermediate learner.\n",
                "Rules:\n",
                "- Use clear, natural language. Sentences can be moderate length.\n",
                "- Avoid rare words, idioms, and overly complex structures.\n",
                "- Keep the full story and most details. Simplify only where needed.\n",
            ),
            "B2" => concat!(
                "You are writing for an upper-intermediate learner.\n",
                "Rules:\n",
                "- Preserve the original style and narrative closely.\n",
                "- Simplify only unusually complex sentences and rare vocabulary.\n",
                "- Replace C1/C2 words with B2-level equivalents where possible.\n",
            ),
            _ => "Rewrite faithfully, preserving the original style and vocabulary.\n",
        };

        format!(
            "Rewrite the following {lang} text at CEFR level {level}.\n\
             Keep the text in {lang} — do NOT translate it.\n\n\
             {level_instructions}\n\
             Do NOT include any introduction, explanation, or commentary.\n\
             Do NOT write sentences like \"Here is the simplified text\".\n\
             Do NOT repeat any sentence or phrase you have already written.\n\
             Each sentence must say something new.\n\
             Output ONLY the rewritten {lang} text, nothing else.\n\nText:\n{text}"
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
        let prompt = Self::build_prompt(text, source_lang, target_lang, level, None, self.simple_prompt);

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
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| TranslateError::ApiError("empty or null content in API response".to_string()))?;
        Ok(content.to_string())
    }

    async fn simplify_chunk(
        &self,
        text: &str,
        lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = Self::build_simplify_prompt(text, lang, level, self.simple_prompt);

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
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| TranslateError::ApiError("empty or null content in API response".to_string()))?;
        Ok(content.to_string())
    }
}

pub struct OllamaTranslator {
    pub base_url: String,
    pub model: String,
    pub simple_prompt: bool,
    client: reqwest::Client,
}

impl OllamaTranslator {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".into()),
            model: model.unwrap_or_else(|| "llama3.1:8b".into()),
            simple_prompt: false,
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
        let prompt = GroqTranslator::build_prompt(text, source_lang, target_lang, level, None, self.simple_prompt);

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
        let content = body["response"]
            .as_str()
            .ok_or_else(|| TranslateError::ApiError("empty or null response field in Ollama response".to_string()))?;
        Ok(content.to_string())
    }

    async fn simplify_chunk(
        &self,
        text: &str,
        lang: &str,
        level: &str,
    ) -> Result<String, TranslateError> {
        let prompt = GroqTranslator::build_simplify_prompt(text, lang, level, self.simple_prompt);

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
        let content = body["response"]
            .as_str()
            .ok_or_else(|| TranslateError::ApiError("empty or null response field in Ollama response".to_string()))?;
        Ok(content.to_string())
    }
}

/// Returns true if the given CEFR level benefits from the two-pass pipeline
/// (separate simplification + translation passes). A1 and A2 require aggressive
/// structural simplification that is best done in the source language first.
/// B1 and above are simplified well enough in a single combined pass.
pub fn requires_two_pass(level: &str) -> bool {
    matches!(level, "A1" | "A2")
}

/// Translate all chunks sequentially.
///
/// When `two_pass` is true (recommended for A1/A2):
///   Pass 1: `simplifier` rewrites each chunk in the source language at the target CEFR level.
///   Pass 2: `translator` translates the simplified text to the target language faithfully.
///
/// When `two_pass` is false (B1 and above):
///   Single pass: `translator` simplifies and translates in one step.
///
/// Retries up to 3 times per pass with exponential backoff.
/// On rate-limit errors (429), waits 30s before retrying.
pub async fn translate_chunks(
    simplifier: &dyn Translator,
    translator: &dyn Translator,
    chunks: &[crate::book::Chunk],
    source_lang: &str,
    target_lang: &str,
    level: &str,
    two_pass: bool,
) -> Result<Vec<String>, TranslateError> {
    use std::io::Write;
    let total = chunks.len();
    let mut results = Vec::new();

    for (i, chunk) in chunks.iter().enumerate() {
        let translated = if two_pass {
            // --- Pass 1: simplify in source language ---
            print!(
                "\r  [{}/{}] chapter {} chunk {} (simplify)   ",
                i + 1, total, chunk.chapter_index + 1, chunk.chunk_index + 1
            );
            let _ = std::io::stdout().flush();

            let mut last_err = None;
            let mut simplified = String::new();
            for attempt in 0..3u32 {
                match simplifier.simplify_chunk(&chunk.content, source_lang, level).await {
                    Ok(t) => { simplified = t; last_err = None; break; }
                    Err(e) => {
                        let is_rate_limit = matches!(&e, TranslateError::ApiError(s) if s.starts_with("429"));
                        let wait_secs = if is_rate_limit { 30u64 } else { 2u64.pow(attempt) };
                        last_err = Some(e);
                        if attempt < 2 { tokio::time::sleep(Duration::from_secs(wait_secs)).await; }
                    }
                }
            }
            if let Some(e) = last_err { return Err(e); }

            // Same-language: Pass 1 output is already in target language — skip translation pass
            if source_lang == target_lang {
                results.push(simplified);
                continue;
            }

            // --- Pass 2: translate simplified text ---
            print!(
                "\r  [{}/{}] chapter {} chunk {} (translate)  ",
                i + 1, total, chunk.chapter_index + 1, chunk.chunk_index + 1
            );
            let _ = std::io::stdout().flush();

            let mut last_err = None;
            let mut result = String::new();
            for attempt in 0..3u32 {
                match translator.translate_chunk(&simplified, source_lang, target_lang, "").await {
                    Ok(t) => { result = t; last_err = None; break; }
                    Err(e) => {
                        let is_rate_limit = matches!(&e, TranslateError::ApiError(s) if s.starts_with("429"));
                        let wait_secs = if is_rate_limit { 30u64 } else { 2u64.pow(attempt) };
                        last_err = Some(e);
                        if attempt < 2 { tokio::time::sleep(Duration::from_secs(wait_secs)).await; }
                    }
                }
            }
            if let Some(e) = last_err { return Err(e); }
            result
        } else {
            // --- Single pass ---
            print!(
                "\r  [{}/{}] chapter {} chunk {}   ",
                i + 1, total, chunk.chapter_index + 1, chunk.chunk_index + 1
            );
            let _ = std::io::stdout().flush();

            let mut last_err = None;
            let mut result = String::new();
            for attempt in 0..3u32 {
                // Same-language: use simplify prompt; cross-language: combined simplify+translate
                let call = if source_lang == target_lang {
                    simplifier.simplify_chunk(&chunk.content, source_lang, level).await
                } else {
                    translator.translate_chunk(&chunk.content, source_lang, target_lang, level).await
                };
                match call {
                    Ok(t) => { result = t; last_err = None; break; }
                    Err(e) => {
                        let is_rate_limit = matches!(&e, TranslateError::ApiError(s) if s.starts_with("429"));
                        let wait_secs = if is_rate_limit { 30u64 } else { 2u64.pow(attempt) };
                        last_err = Some(e);
                        if attempt < 2 { tokio::time::sleep(Duration::from_secs(wait_secs)).await; }
                    }
                }
            }
            if let Some(e) = last_err { return Err(e); }
            result
        };

        results.push(translated);
    }
    println!();
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
        let p = GroqTranslator::build_prompt("Hello world.", "en", "de", "A2", None, false);
        assert!(
            p.contains("en") && p.contains("de") && p.contains("A2") && p.contains("Hello world.")
        );
    }

    #[test]
    fn prompt_includes_context_when_provided() {
        let ctx = "some previous context";
        let p = GroqTranslator::build_prompt("New text.", "en", "de", "A2", Some(ctx), false);
        assert!(p.contains(ctx));
        assert!(p.contains("New text."));
    }

    #[test]
    fn simplify_prompt_keeps_same_language() {
        let p = GroqTranslator::build_simplify_prompt("Hallo Welt.", "German", "A1", false);
        assert!(p.contains("German"));
        assert!(p.contains("A1"));
        assert!(p.contains("Hallo Welt."));
        // Must instruct the model to keep the language, not translate to another language
        assert!(p.contains("Keep the text in German") || p.contains("do NOT translate"));
        // Must NOT instruct "Translate the following" as a primary directive
        assert!(!p.contains("Translate the following"));
    }

    #[test]
    fn simplify_prompt_contains_anti_repetition() {
        let p = GroqTranslator::build_simplify_prompt("some text", "English", "A2", false);
        assert!(p.contains("Do NOT repeat any sentence"));
        assert!(p.contains("Each sentence must say something new"));
    }

    #[test]
    fn simplify_prompt_simple_mode() {
        let p = GroqTranslator::build_simplify_prompt("Bonjour.", "French", "B1", true);
        assert!(p.contains("French"));
        assert!(p.contains("B1"));
        assert!(p.contains("Bonjour."));
    }

    #[test]
    fn ollama_defaults() {
        let t = OllamaTranslator::new(None, None);
        assert_eq!(t.base_url, "http://localhost:11434");
        assert_eq!(t.model, "llama3.1:8b");
    }

    struct MockTranslator {
        responses: std::sync::Mutex<Vec<Result<String, TranslateError>>>,
    }

    impl MockTranslator {
        fn new(responses: Vec<Result<String, TranslateError>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl Translator for MockTranslator {
        async fn translate_chunk(
            &self, _text: &str, _src: &str, _tgt: &str, _level: &str,
        ) -> Result<String, TranslateError> {
            self.responses.lock().unwrap().remove(0)
        }
    }

    #[tokio::test]
    async fn translate_chunks_empty_input() {
        let translator = MockTranslator::new(vec![]);
        let result = translate_chunks(&translator, &translator, &[], "en", "de", "A2", false).await;
        assert!(matches!(result, Ok(v) if v.is_empty()));
    }

    #[tokio::test]
    async fn translate_chunks_propagates_error() {
        // Two-pass: pass 1 (simplify) uses default impl -> translate_chunk,
        // pass 2 (translate) also uses translate_chunk.
        // We need 3 failures for pass 1 to exhaust retries.
        let translator = MockTranslator::new(vec![
            Err(TranslateError::ApiError("500: error".to_string())),
            Err(TranslateError::ApiError("500: error".to_string())),
            Err(TranslateError::ApiError("500: error".to_string())),
        ]);
        let chunks = vec![crate::book::Chunk {
            chapter_index: 0,
            chunk_index: 0,
            content: "text".into(),
        }];
        let result = translate_chunks(&translator, &translator, &chunks, "en", "de", "A2", false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn translate_chunks_passes_context_to_second_chunk() {
        // Two-pass per chunk: each chunk needs 2 successful translate_chunk calls
        // (simplify default impl + translate pass).
        let translator = MockTranslator::new(vec![
            Ok("simplified first chunk".to_string()),          // chunk 0, pass 1 (simplify)
            Ok("translated first chunk with unique words".to_string()), // chunk 0, pass 2 (translate)
            Ok("simplified second chunk".to_string()),         // chunk 1, pass 1 (simplify)
            Ok("translated second chunk".to_string()),         // chunk 1, pass 2 (translate)
        ]);
        let chunks = vec![
            crate::book::Chunk {
                chapter_index: 0,
                chunk_index: 0,
                content: "first".into(),
            },
            crate::book::Chunk {
                chapter_index: 0,
                chunk_index: 1,
                content: "second".into(),
            },
        ];
        let result = translate_chunks(&translator, &translator, &chunks, "en", "de", "A1", true)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "translated first chunk with unique words");
        assert_eq!(result[1], "translated second chunk");
    }
}
