use std::time::SystemTime;

use rocket::futures::{StreamExt, FutureExt};
use simon_cadge_klu_ai_be_exercise::{json_parsing::{build_conversations_data_from_file, Role, Message}, http_parsing::{ChatCompletionRequest, ChatCompletionResponse}};

/// Simple reqwest client designed to sanity check and benchmark the main server.
/// It parses the JSON file the same as the main server, and then asynchronously iterates through every conversation,
/// sending a post request to the server for every User message that it finds. It panics on any error, be that an error with the
/// tokio asynchronous stream handling, a reqwest connection error, or a 404 not found error returned from the server.
/// For every response successfully received, the received message is compared against the expected message.
/// If the received message doesn't match there is a chance that the request was one of a number of conversations in the JSON document
/// which begin with an identical User message (e.g. 'hi'), so it gets the conversation id from the result and checks that id in the
/// parsed conversations object to ensure that the response is indeed valid. If this assert fails then the client also panics.
/// Assuming every single request completes successfuly and none of the asserts fail, the client will print the total number of
/// requests and the elapsed time.
#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let available_parallelism = std::thread::available_parallelism().unwrap();

    log::debug!("Available Parralelism: {}", available_parallelism);

    let conversations = build_conversations_data_from_file().unwrap();

    let client = reqwest::Client::new();

    let stream = conversations.stream_conversations();

    log::info!("Starting requests");
    let start = SystemTime::now();

    //Async stream every conversation
    let request_count = stream.flat_map_unordered(1, |conversation| {
        //A single conversation might trigger multiple requests, so flat map concat their join handles
        let mut join_handles = Vec::new();
        for (index, message) in conversation.iter().enumerate() {
            //For a given Assistant message, make a ChatCompletionRequest with every message leading to this one and post it to the server
            if message.role == Role::Assistant {
                let request = ChatCompletionRequest { 
                    messages: conversation[0..index].to_vec()
                };
                let client = client.clone();
                let expected_response = conversation[index].content.clone();
                join_handles.push(tokio::spawn(async move {
                    let response = client.post("http://127.0.0.1:8000/v1/chat/completions")
                        .json(&request)
                        .send()
                        .await
                        .unwrap();
                    //Turn an error status code into a reqwest error
                    match response.error_for_status() {
                        //If status code is good, wrap together actual response with expected response, to be asserted later.
                        Ok(res) => Ok(res.json::<ChatCompletionResponse>()
                            .then(move |text| async move {
                                text.map(|string| (string, expected_response))
                            })
                            .await
                        ),
                        //If status code is error, return the error
                        Err(err) => Err(err)
                    }
                }));
            }
        }
        tokio_stream::iter(join_handles)
    })
    //Asynchronously process as many requests as supported on the current hardware
    .buffer_unordered(available_parallelism.into())
    .then(|result| async {
        //Panic on any errors, or assert that the response message matches the expected response
        match result {
            Ok(Ok(Ok((result, expected)))) => {
                if result.message.content != expected {
                    assert!(conversations.get_conversation(&result.id).unwrap().contains(&Message { role: Role::Assistant, content: result.message.content }), "Result didn't match expected result and also didn't match returned result");
                }
            },
            Ok(Ok(Err(e))) => panic!("Request status error: {}", e),
            Ok(Err(e)) => panic!("Reqwest error: {}", e),
            Err(e) => panic!("Tokio error: {}", e),
        }
    })
    .count()
    .await;

    let elapsed_time = start.elapsed().expect("Time went backwards");

    log::info!("Processed {} requests in {} seconds", request_count, elapsed_time.as_secs_f64());
}