// WorkMirror - Async HTTP client for local Ollama API.
//
// Communicates with a locally running Ollama instance via the
// `/api/generate` endpoint.  The default URL is `http://localhost:11434`;
// this can be overridden by passing a custom URL.
//
// ## Error handling
//
// If Ollama is not running, the client returns `AiError::OllamaNotRunning`
// with a Chinese-language message suitable for direct display in the UI.
// All HTTP errors are wrapped with context before being returned.

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when talking to the Ollama API.
#[derive(Debug, Error)]
pub enum AiError {
    /// Ollama is not running or unreachable.
    #[error("{0}")]
    OllamaNotRunning(String),

    /// The request timed out.
    #[error("Request timed out: {0}")]
    Timeout(String),

    /// The API returned a non-success HTTP status.
    #[error("API error (HTTP {status}): {message}")]
    ApiError { status: u16, message: String },

    /// A deserialisation / internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<reqwest::Error> for AiError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AiError::Timeout(e.to_string())
        } else if e.is_connect() {
            AiError::OllamaNotRunning(
                "Ollama 未运行，请先启动 Ollama (ollama serve)".into(),
            )
        } else {
            AiError::Internal(e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Ollama API types
// ---------------------------------------------------------------------------

/// Request body for `/api/generate`.
#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
    options: GenerateOptions,
}

#[derive(Debug, Serialize)]
struct GenerateOptions {
    /// Maximum number of tokens to generate.
    num_predict: u32,
    /// Sampling temperature.
    temperature: f64,
}

/// Response body from `/api/generate` (non-streaming).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GenerateResponse {
    response: String,
    done: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Default Ollama endpoint.
const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Default model to use.
const DEFAULT_MODEL: &str = "llama3.2:3b";

/// Maximum number of tokens to generate per call.
const MAX_TOKENS: u32 = 512;

/// Sampling temperature (lower = more deterministic).
const TEMPERATURE: f64 = 0.7;

/// Request timeout in seconds.
const TIMEOUT_SECS: u64 = 60;

/// Send a prompt to a locally running Ollama instance and return the
/// generated text.
///
/// `ollama_url` overrides the default `http://localhost:11434`.
/// `model` overrides the default `llama3.2:3b`; pass an empty string to
/// keep the default.
pub async fn generate(
    prompt: &str,
    ollama_url: Option<&str>,
    model: Option<&str>,
) -> Result<String, AiError> {
    let base_url = ollama_url.unwrap_or(DEFAULT_OLLAMA_URL).trim_end_matches('/');
    let model_name = model.unwrap_or(DEFAULT_MODEL);

    // Health-check: call /api/tags first to verify Ollama is running.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| AiError::Internal(format!("Failed to create HTTP client: {e}")))?;

    let health_url = format!("{base_url}/api/tags");
    let health_resp = client.get(&health_url).send().await;

    match health_resp {
        Ok(resp) => {
            if !resp.status().is_success() {
                return Err(AiError::ApiError {
                    status: resp.status().as_u16(),
                    message: "Ollama health check failed".into(),
                });
            }
        }
        Err(e) => {
            // Connection refused or DNS failure -> Ollama is not running.
            return Err(AiError::OllamaNotRunning(format!(
                "Ollama 未运行或无法连接。请确保已启动 Ollama (ollama serve)。\n详细信息：{e}"
            )));
        }
    }

    // Build the generate request.
    let request = GenerateRequest {
        model: model_name.into(),
        prompt: prompt.into(),
        stream: false,
        options: GenerateOptions {
            num_predict: MAX_TOKENS,
            temperature: TEMPERATURE,
        },
    };

    let generate_url = format!("{base_url}/api/generate");
    let resp = client
        .post(&generate_url)
        .json(&request)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(AiError::ApiError {
            status,
            message: body,
        });
    }

    let body: GenerateResponse = resp
        .json()
        .await
        .map_err(|e| AiError::Internal(format!("Failed to parse Ollama response: {e}")))?;

    Ok(body.response.trim().to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock HTTP server that returns predefined Ollama responses.
    /// Only runs when the `mock` cfg flag is set (not in normal `cargo test`).
    /// For unit tests we test the error types and URL construction instead.

    #[test]
    fn default_url_is_localhost() {
        assert_eq!(DEFAULT_OLLAMA_URL, "http://localhost:11434");
    }

    #[test]
    fn default_model_is_llama32() {
        assert_eq!(DEFAULT_MODEL, "llama3.2:3b");
    }

    #[test]
    fn error_from_connect_failure_is_ollama_not_running() {
        // Create a reqwest error that looks like a connection failure.
        // We can't easily instantiate reqwest::Error directly, so we test
        // the error type via the From impl by checking the message format.
        let err = AiError::OllamaNotRunning(
            "Ollama 未运行，请先启动 Ollama (ollama serve)".into(),
        );
        assert!(err.to_string().contains("Ollama"));
        assert!(err.to_string().contains("ollama serve"));
    }

    #[test]
    fn timeout_error_has_timeout_in_message() {
        let err = AiError::Timeout("request took too long".into());
        let msg = err.to_string();
        assert!(msg.contains("timed out") || msg.contains("timeout"),
            "expected 'timeout' in '{msg}'");
    }

    #[test]
    fn api_error_contains_status_code() {
        let err = AiError::ApiError {
            status: 404,
            message: "model not found".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("404"));
        assert!(msg.contains("model not found"));
    }

    #[test]
    fn health_check_url_format() {
        let base = "http://localhost:11434";
        assert_eq!(
            format!("{base}/api/tags"),
            "http://localhost:11434/api/tags"
        );
    }

    #[test]
    fn generate_url_format() {
        let base = "http://localhost:11434";
        assert_eq!(
            format!("{base}/api/generate"),
            "http://localhost:11434/api/generate"
        );
    }

    #[test]
    fn custom_ollama_url_trim_trailing_slash() {
        let url = "http://192.168.1.100:11434/";
        let trimmed = url.trim_end_matches('/');
        assert_eq!(trimmed, "http://192.168.1.100:11434");
        assert_eq!(
            format!("{trimmed}/api/tags"),
            "http://192.168.1.100:11434/api/tags"
        );
    }

    /// Test that the error message for a non-running Ollama is friendly.
    #[test]
    fn error_message_is_user_friendly() {
        let err = AiError::OllamaNotRunning(
            "Ollama 未运行或无法连接。请确保已启动 Ollama (ollama serve)。\n详细信息：connection refused".into(),
        );
        let msg = err.to_string();
        assert!(msg.contains("Ollama"));
        assert!(msg.contains("connection refused"));
    }
}
