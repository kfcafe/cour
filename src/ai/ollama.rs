use serde::{Deserialize, Serialize};

use crate::ai::traits::{Embedder, EmbeddingResult};
use crate::config::{AppConfig, ProviderConfig};
use crate::error::{AppError, AppResult};

const DEFAULT_OLLAMA_EMBEDDINGS_PATH: &str = "/api/embeddings";

#[derive(Debug, Clone)]
pub struct OllamaEmbedder {
    provider: String,
    model: String,
    api_url: String,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn from_config(config: &AppConfig) -> AppResult<Self> {
        let provider = config
            .ai
            .embedding
            .as_ref()
            .ok_or_else(|| AppError::Ai("missing embedding provider config".to_string()))?;
        Self::from_provider_config(provider)
    }

    fn from_provider_config(provider: &ProviderConfig) -> AppResult<Self> {
        let api_url = provider
            .api_url
            .as_deref()
            .ok_or_else(|| AppError::Ai("missing Ollama api_url".to_string()))?;

        Ok(Self {
            provider: provider.provider.clone(),
            model: provider.model.clone(),
            api_url: Self::normalize_api_url(api_url),
            client: reqwest::Client::new(),
        })
    }

    fn normalize_api_url(api_url: &str) -> String {
        let trimmed = api_url.trim_end_matches('/');
        if trimmed.ends_with(DEFAULT_OLLAMA_EMBEDDINGS_PATH) {
            trimmed.to_string()
        } else {
            format!("{trimmed}{DEFAULT_OLLAMA_EMBEDDINGS_PATH}")
        }
    }

    pub fn build_embedding_input(subject: Option<&str>, body_text: &str) -> String {
        match subject {
            Some(subject) if !subject.trim().is_empty() => {
                format!("Subject: {}\n\n{}", subject.trim(), body_text.trim())
            }
            _ => body_text.trim().to_string(),
        }
    }

    fn build_embedding_request(&self, text: &str) -> EmbeddingApiRequest {
        EmbeddingApiRequest {
            model: self.model.clone(),
            prompt: text.to_string(),
        }
    }

    fn parse_embedding_response(
        &self,
        response: EmbeddingApiResponse,
    ) -> AppResult<EmbeddingResult> {
        if response.embedding.is_empty() {
            return Err(AppError::Ai(
                "Ollama returned an empty embedding".to_string(),
            ));
        }

        Ok(EmbeddingResult {
            provider: self.provider.clone(),
            model: self.model.clone(),
            vector: response.embedding,
        })
    }
}

#[async_trait::async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> AppResult<EmbeddingResult> {
        let request = self.build_embedding_request(text);
        let response = self
            .client
            .post(&self.api_url)
            .json(&request)
            .send()
            .await
            .map_err(|err| AppError::Ai(format!("failed to call Ollama embeddings API: {err}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable response body>".to_string());
            return Err(AppError::Ai(format!(
                "Ollama embeddings API request failed with status {status}: {body}"
            )));
        }

        let payload = response
            .json::<EmbeddingApiResponse>()
            .await
            .map_err(|err| {
                AppError::Ai(format!("failed to decode Ollama embedding response: {err}"))
            })?;

        self.parse_embedding_response(payload)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbeddingApiRequest {
    model: String,
    prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
struct EmbeddingApiResponse {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use crate::config::{AiConfig, AppConfig, ProviderConfig};

    use super::{EmbeddingApiResponse, OllamaEmbedder};

    fn sample_config() -> AppConfig {
        AppConfig {
            accounts: vec![],
            ai: AiConfig {
                embedding: Some(ProviderConfig {
                    provider: "ollama".to_string(),
                    model: "nomic-embed-text".to_string(),
                    api_url: Some("http://localhost:11434/api/embeddings".to_string()),
                    api_key_env: None,
                    enabled: Some(true),
                }),
                extraction: None,
                drafting: None,
            },
            smtp: vec![],
        }
    }

    #[test]
    fn parses_embedding_response_fixture() {
        let embedder = OllamaEmbedder::from_config(&sample_config()).expect("embedder from config");
        let response: EmbeddingApiResponse =
            serde_json::from_str(r#"{"embedding":[0.1,0.2,0.3]}"#).expect("parse response fixture");
        let result = embedder
            .parse_embedding_response(response)
            .expect("parse embedding response");
        assert_eq!(result.vector, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn build_embedding_input_includes_subject() {
        let text = OllamaEmbedder::build_embedding_input(Some("Hello"), "World");
        assert!(text.contains("Subject: Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn build_embedding_request_uses_model_and_prompt() {
        let embedder = OllamaEmbedder::from_config(&sample_config()).expect("embedder from config");
        let request = embedder.build_embedding_request("hello world");

        let payload = serde_json::to_value(&request).expect("serialize request");
        assert_eq!(payload["model"], "nomic-embed-text");
        assert_eq!(payload["prompt"], "hello world");
    }

    #[test]
    fn appends_embeddings_path_to_base_url() {
        let mut config = sample_config();
        config
            .ai
            .embedding
            .as_mut()
            .expect("embedding config")
            .api_url = Some("http://localhost:11434".to_string());

        let embedder = OllamaEmbedder::from_config(&config).expect("embedder from config");
        assert_eq!(embedder.api_url, "http://localhost:11434/api/embeddings");
    }
}
