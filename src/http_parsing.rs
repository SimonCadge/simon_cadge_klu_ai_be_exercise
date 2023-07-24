use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::json_parsing::Message;

/// Struct for serde to deserialize a chat completion request into, matching the openai api.
#[derive(Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<Message>,
}

impl ChatCompletionRequest {
    pub fn hash(&self) -> String {
        let mut hash_string = String::from("");
        for message in &self.messages {
            hash_string.push_str(&message.content);
        }
        return hash_string;
    }
}

/// Struct to serialize the chat completion response returned in the response body.
#[derive(Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    #[serde(with = "time::serde::timestamp")]
    pub created: OffsetDateTime,
    pub message: Message
}