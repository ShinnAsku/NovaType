//! LLM backend abstraction with timeout-based circuit breaking.
//!
//! The engine never depends on an LLM: every call site must treat failures
//! and timeouts as "no result" and fall back to local candidates.

use serde::Deserialize;
use std::fmt;
use std::time::Duration;

/// Error type for LLM calls.
#[derive(Debug)]
pub struct LlmError(String);

impl fmt::Display for LlmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "llm error: {}", self.0)
    }
}

impl std::error::Error for LlmError {}

pub type LlmResult<T> = Result<T, LlmError>;

/// A pluggable LLM completion backend.
pub trait LlmBackend: Send + Sync {
    /// Completes `prompt`, respecting the configured timeout.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is unreachable, times out, or
    /// returns an invalid response. Callers must degrade gracefully.
    fn complete(&self, prompt: &str) -> LlmResult<String>;

    /// Human-readable backend name for diagnostics.
    fn name(&self) -> &'static str;
}

/// Ollama HTTP backend (`/api/generate`).
pub struct OllamaBackend {
    base_url: String,
    model: String,
    timeout: Duration,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
}

impl OllamaBackend {
    /// Creates a backend against a local or remote Ollama instance.
    #[must_use]
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            timeout,
        }
    }

    /// Creates a backend against the default local Ollama endpoint.
    #[must_use]
    pub fn local(model: impl Into<String>) -> Self {
        Self::new("http://127.0.0.1:11434", model, Duration::from_secs(10))
    }

    /// The generate endpoint URL.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("{}/api/generate", self.base_url)
    }
}

impl LlmBackend for OllamaBackend {
    fn complete(&self, prompt: &str) -> LlmResult<String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(500))
            .timeout(self.timeout)
            .build();

        let response = agent
            .post(&self.endpoint())
            .send_json(ureq::json!({
                "model": self.model,
                "prompt": prompt,
                "stream": false,
            }))
            .map_err(|error| LlmError(error.to_string()))?;

        let parsed: OllamaResponse = response
            .into_json()
            .map_err(|error| LlmError(error.to_string()))?;
        Ok(parsed.response.trim().to_string())
    }

    fn name(&self) -> &'static str {
        "ollama"
    }
}

#[cfg(test)]
mod tests {
    use super::{LlmBackend, OllamaBackend};
    use std::time::Duration;

    #[test]
    fn builds_endpoint_from_base_url() {
        let backend = OllamaBackend::new(
            "http://127.0.0.1:11434/",
            "qwen2:0.5b",
            Duration::from_secs(1),
        );

        assert_eq!(backend.endpoint(), "http://127.0.0.1:11434/api/generate");
        assert_eq!(backend.name(), "ollama");
    }

    #[test]
    fn unreachable_backend_fails_fast() {
        // Port 9 (discard) is never an Ollama server; this must error, not hang.
        let backend = OllamaBackend::new("http://127.0.0.1:9", "test", Duration::from_millis(300));

        let result = backend.complete("hello");
        assert!(result.is_err());
    }
}
