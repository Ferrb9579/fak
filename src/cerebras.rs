use rig::OneOrMany;
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::{
    AssistantContent, CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    Message,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CerebrasClient {
    api_key: String,
    base_url: String,
    http_client: reqwest::Client,
}

impl CerebrasClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.cerebras.ai/v1".to_string(),
            http_client: reqwest::Client::new(),
        }
    }
}

#[derive(Debug)]
pub enum CerebrasError {
    ApiKeyNotFound,
    HttpError(reqwest::Error),
    RequestError(String),
}

impl std::fmt::Display for CerebrasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CerebrasError::ApiKeyNotFound => {
                write!(f, "CEREBRAS_API_KEY environment variable not found")
            }
            CerebrasError::HttpError(err) => write!(f, "HTTP error: {}", err),
            CerebrasError::RequestError(msg) => write!(f, "Request error: {}", msg),
        }
    }
}

impl std::error::Error for CerebrasError {}

impl ProviderClient for CerebrasClient {
    type Input = String;
    type Error = CerebrasError;

    fn from_env() -> Result<Self, Self::Error> {
        let api_key =
            std::env::var("CEREBRAS_API_KEY").map_err(|_| CerebrasError::ApiKeyNotFound)?;
        Ok(Self::new(api_key))
    }

    fn from_val(input: Self::Input) -> Result<Self, Self::Error> {
        Ok(Self::new(input))
    }
}

impl CompletionClient for CerebrasClient {
    type CompletionModel = CerebrasCompletionModel;

    fn completion_model(&self, model: impl Into<String>) -> Self::CompletionModel {
        CerebrasCompletionModel {
            client: self.clone(),
            model: model.into(),
        }
    }
}

#[derive(Clone)]
pub struct CerebrasCompletionModel {
    client: CerebrasClient,
    model: String,
}

#[derive(Serialize)]
struct CerebrasRequestMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct CerebrasCompletionRequest {
    model: String,
    messages: Vec<CerebrasRequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CerebrasCompletionResponse {
    pub id: Option<String>,
    pub choices: Vec<CerebrasChoice>,
    pub usage: Option<CerebrasUsage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CerebrasChoice {
    pub index: Option<usize>,
    pub message: CerebrasResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CerebrasResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CerebrasUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl CompletionModel for CerebrasCompletionModel {
    type Response = CerebrasCompletionResponse;
    type StreamingResponse = ();
    type Client = CerebrasClient;

    fn make(client: &Self::Client, model: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            model: model.into(),
        }
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        let mut messages = Vec::new();

        if let Some(preamble) = request.preamble {
            messages.push(CerebrasRequestMessage {
                role: "system",
                content: preamble,
            });
        }

        for msg in request.chat_history {
            match msg {
                Message::System { content } => {
                    messages.push(CerebrasRequestMessage {
                        role: "system",
                        content,
                    });
                }
                Message::User { content } => {
                    let mut text = String::new();
                    for part in content.iter() {
                        match part {
                            rig::message::UserContent::Text(t) => {
                                text.push_str(&t.text);
                            }
                            _ => {}
                        }
                    }
                    messages.push(CerebrasRequestMessage {
                        role: "user",
                        content: text,
                    });
                }
                Message::Assistant { content, .. } => {
                    let mut text = String::new();
                    for part in content.iter() {
                        match part {
                            AssistantContent::Text(t) => {
                                text.push_str(&t.text);
                            }
                            _ => {}
                        }
                    }
                    messages.push(CerebrasRequestMessage {
                        role: "assistant",
                        content: text,
                    });
                }
            }
        }

        let body = CerebrasCompletionRequest {
            model: self.model.clone(),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

        let response = self
            .client
            .http_client
            .post(format!("{}/chat/completions", self.client.base_url))
            .header("Authorization", format!("Bearer {}", self.client.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| CompletionError::RequestError(Box::new(e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CompletionError::from_http_response(status, body));
        }

        let res_body: CerebrasCompletionResponse = response
            .json()
            .await
            .map_err(|e| CompletionError::ResponseError(e.to_string()))?;

        let choice_text = res_body
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let choice = OneOrMany::one(AssistantContent::text(choice_text));

        let usage = rig::completion::Usage {
            input_tokens: res_body
                .usage
                .as_ref()
                .map_or(0, |u| u.prompt_tokens as u64),
            output_tokens: res_body
                .usage
                .as_ref()
                .map_or(0, |u| u.completion_tokens as u64),
            total_tokens: res_body.usage.as_ref().map_or(0, |u| u.total_tokens as u64),
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        };

        Ok(CompletionResponse {
            choice,
            usage,
            raw_response: res_body.clone(),
            message_id: res_body.id,
        })
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<rig::streaming::StreamingCompletionResponse<Self::StreamingResponse>, CompletionError>
    {
        Err(CompletionError::RequestError(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Streaming is not supported for Cerebras client",
            ),
        )))
    }
}
