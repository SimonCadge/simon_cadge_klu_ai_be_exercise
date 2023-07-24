use std::sync::Arc;

use rocket::{serde::json::Json, State, response::status::NotFound};
use simon_cadge_klu_ai_be_exercise::{http_parsing::{ChatCompletionRequest, ChatCompletionResponse}, json_parsing::{build_conversations_data_from_file, preprocess_hashed_responses, HashedResponses}};
use time::OffsetDateTime;

#[macro_use] extern crate rocket;

/// Listen to post requests at the /v1/chat/completions endpoint which provide chat completion request data in the request body.
/// The request body is automatically parsed using serde and an error message is returned if the body is formatted incorrectly.
#[post("/v1/chat/completions", format = "json", data = "<chat_completion_request>")]
fn handle_chat_completion_request(chat_completion_request: Json<ChatCompletionRequest>, hashed_responses: &State<Arc<HashedResponses>>) -> Result<Json<ChatCompletionResponse>, NotFound<String>> {
    //Search for a valid response for the given request.
    let response = match hashed_responses.get_response_for_request(&chat_completion_request) {
        Some(response) => Ok(rocket::serde::json::Json(ChatCompletionResponse { //If any valid response found, return the assistant message along with associated conversation id and a timestamp.
                id: response.id.clone(),
                created: OffsetDateTime::now_utc(),
                message: response.response_message.clone()
            })),
        None => Err(NotFound(String::from("No valid response exists for the given request"))) //If no valid response found, return 404 not found error with text describing the error.
    };
    response
}

#[launch]
fn rocket() -> _ {
    //Parse conversations from JSON
    let conversations = build_conversations_data_from_file().unwrap();
    
    //Preprocess conversations into hashed responses
    let responses = preprocess_hashed_responses(&conversations);

    //Run simple rocket server
    rocket::build()
        .manage(responses)
        .mount("/", routes![handle_chat_completion_request])
}