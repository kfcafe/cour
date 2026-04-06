use serde::{Deserialize, Serialize};

use crate::ai::traits::{
    DraftGenerationResult, DraftGenerator, DraftGeneratorInput, ExtractionOutput, Extractor,
    ExtractorInput,
};
use crate::config::{AppConfig, ProviderConfig};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    provider: String,
    model: String,
    api_url: String,
    api_key_env: Option<String>,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn from_extraction_config(config: &AppConfig) -> AppResult<Self> {
        let provider = config
            .ai
            .extraction
            .as_ref()
            .ok_or_else(|| AppError::Ai("missing extraction provider config".to_string()))?;
        Self::from_provider_config(provider)
    }

    pub fn from_drafting_config(config: &AppConfig) -> AppResult<Self> {
        let provider = config
            .ai
            .drafting
            .as_ref()
            .ok_or_else(|| AppError::Ai("missing drafting provider config".to_string()))?;
        Self::from_provider_config(provider)
    }

    fn from_provider_config(provider: &ProviderConfig) -> AppResult<Self> {
        Ok(Self {
            provider: provider.provider.clone(),
            model: provider.model.clone(),
            api_url: provider
                .api_url
                .clone()
                .ok_or_else(|| AppError::Ai("missing OpenAI-compatible api_url".to_string()))?,
            api_key_env: provider.api_key_env.clone(),
            client: reqwest::Client::new(),
        })
    }

    pub fn build_extraction_request(&self, input: &ExtractorInput) -> ChatRequest {
        ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "Extract structured JSON from this thread. Subject: {:?}. Messages: {:?}",
                    input.subject, input.messages
                ),
            }],
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
        }
    }

    pub fn build_draft_request(&self, input: &DraftGeneratorInput) -> ChatRequest {
        ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: format!(
                    "Draft a reply. Subject: {:?}. Latest ask: {:?}. Messages: {:?}. Tone: {:?}",
                    input.subject, input.latest_ask, input.messages, input.tone_hint
                ),
            }],
            response_format: Some(ResponseFormat {
                format_type: "json_object".to_string(),
            }),
        }
    }

    fn parse_extraction_response(&self, response: ChatResponse) -> AppResult<ExtractionOutput> {
        let content = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Ai("missing extraction choice".to_string()))?
            .message
            .content;

        serde_json::from_str::<ExtractionOutput>(&content)
            .map_err(|err| AppError::Ai(format!("failed to parse extraction response: {err}")))
    }

    fn parse_draft_response(&self, response: ChatResponse) -> AppResult<DraftGenerationResult> {
        let content = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Ai("missing draft choice".to_string()))?
            .message
            .content;

        serde_json::from_str::<DraftGenerationResult>(&content)
            .map_err(|err| AppError::Ai(format!("failed to parse draft response: {err}")))
    }
}

#[async_trait::async_trait]
impl Extractor for OpenAiProvider {
    async fn extract(&self, _input: &ExtractorInput) -> AppResult<ExtractionOutput> {
        Err(AppError::Ai(format!(
            "live OpenAI-compatible extraction not wired yet for {}",
            self.api_url
        )))
    }
}

#[async_trait::async_trait]
impl DraftGenerator for OpenAiProvider {
    async fn generate_draft(
        &self,
        _input: &DraftGeneratorInput,
    ) -> AppResult<DraftGenerationResult> {
        Err(AppError::Ai(format!(
            "live OpenAI-compatible drafting not wired yet for {}",
            self.api_url
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use crate::config::{AiConfig, AppConfig, ProviderConfig};

    use super::{ChatResponse, OpenAiProvider};

    fn sample_config() -> AppConfig {
        AppConfig {
            accounts: vec![],
            ai: AiConfig {
                embedding: None,
                extraction: Some(ProviderConfig {
                    provider: "openai-compatible".to_string(),
                    model: "gpt-4o-mini".to_string(),
                    api_url: Some("https://example.invalid/v1/chat/completions".to_string()),
                    api_key_env: Some("MAILFOR_API_KEY".to_string()),
                    enabled: Some(true),
                }),
                drafting: Some(ProviderConfig {
                    provider: "openai-compatible".to_string(),
                    model: "gpt-4o-mini".to_string(),
                    api_url: Some("https://example.invalid/v1/chat/completions".to_string()),
                    api_key_env: Some("MAILFOR_API_KEY".to_string()),
                    enabled: Some(true),
                }),
            },
            smtp: vec![],
        }
    }

    #[test]
    fn parses_extraction_response_fixture() {
        let provider =
            OpenAiProvider::from_extraction_config(&sample_config()).expect("provider from config");
        let response: ChatResponse = serde_json::from_str(
            r#"{
                "choices": [
                    {
                        "message": {
                            "content": "{\"provider\":\"openai-compatible\",\"model\":\"gpt-4o-mini\",\"summary\":\"Need to reply to Alice.\",\"action\":\"respond\",\"urgency_score\":0.7,\"confidence\":0.9,\"categories\":[\"correspondence\"],\"entities\":[\"Alice\"],\"deadlines\":[\"Friday\"],\"thread_state_hint\":\"waiting_on_me\",\"latest_ask\":\"Reply with pricing\"}"
                        }
                    }
                ]
            }"#,
        )
        .expect("parse fixture");

        let extraction = provider
            .parse_extraction_response(response)
            .expect("parse extraction");
        assert_eq!(extraction.action, "respond");
        assert_eq!(extraction.latest_ask.as_deref(), Some("Reply with pricing"));
    }
}
