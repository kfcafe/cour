use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractMessageInput {
    pub from_email: Option<String>,
    pub subject: Option<String>,
    pub body_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractorInput {
    pub subject: Option<String>,
    pub messages: Vec<ExtractMessageInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractionOutput {
    pub provider: String,
    pub model: String,
    pub summary: String,
    pub action: String,
    pub urgency_score: f32,
    pub confidence: f32,
    pub categories: Vec<String>,
    pub entities: Vec<String>,
    pub deadlines: Vec<String>,
    pub thread_state_hint: Option<String>,
    pub latest_ask: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftGeneratorInput {
    pub subject: Option<String>,
    pub latest_ask: Option<String>,
    pub messages: Vec<ExtractMessageInput>,
    pub tone_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftCandidate {
    pub body: String,
    pub rationale: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftGenerationResult {
    pub provider: String,
    pub model: String,
    pub candidates: Vec<DraftCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResult {
    pub provider: String,
    pub model: String,
    pub vector: Vec<f32>,
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> AppResult<EmbeddingResult>;
}

#[async_trait]
pub trait Extractor: Send + Sync {
    async fn extract(&self, input: &ExtractorInput) -> AppResult<ExtractionOutput>;
}

#[async_trait]
pub trait DraftGenerator: Send + Sync {
    async fn generate_draft(&self, input: &DraftGeneratorInput)
        -> AppResult<DraftGenerationResult>;
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use crate::error::AppResult;

    use super::{Embedder, EmbeddingResult};

    struct StubEmbedder;

    #[async_trait]
    impl Embedder for StubEmbedder {
        async fn embed(&self, text: &str) -> AppResult<EmbeddingResult> {
            Ok(EmbeddingResult {
                provider: "stub".to_string(),
                model: "stub-model".to_string(),
                vector: vec![text.len() as f32],
            })
        }
    }

    #[tokio::test]
    async fn stub_embedder_compiles() {
        let embedder = StubEmbedder;
        let result = embedder.embed("hello").await.expect("embed text");
        assert_eq!(result.vector, vec![5.0]);
    }
}
