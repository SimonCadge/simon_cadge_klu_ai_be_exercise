use std::{fs::File, io::BufReader, collections::HashMap, sync::Arc};

use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Deserializer, Serialize};
use serde::de::{Visitor, SeqAccess};

use std::{fmt, env};
use std::marker::PhantomData;

use crate::http_parsing::ChatCompletionRequest;

/// An enum representing the gpt role.
/// The four actual role names match those defined in the openai chat completions spec.
/// The aliases map the various alternative versions found in the JSON input to the official spec when deserializing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Role {
    #[serde(alias="system")]
    System,
    #[serde(alias="human", alias="user")]
    User,
    #[serde(alias="gpt", alias="bing", alias="chatgpt", alias="bard", alias="assistant")]
    Assistant,
    Function
}

/// A struct representing a single message in a conversation.
/// The titles match those defined in the openai chat completions spec.
/// The aliases map the versions found in the JSON input to the official spec when deserializing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Message{
    #[serde(alias="from")]
    pub role: Role,
    #[serde(alias="value")]
    pub content: String
}

/// Helper struct for use when deserializing the JSON using serde.
#[derive(Deserialize)]
#[serde(transparent)]
struct Outer {
    #[serde(deserialize_with = "parse_convos_into_hashmap")]
    conversations: HashMap<String, Vec<Message>>
}

/// This is the deserialization function which will run in place as the JSON file is read into memory.
/// The original JSON file is a long list of JSON objects, each of which has a string id value and an array of messages called conversations.
/// This function maps the input file into a hashmap where the string id values are the keys and the array of messages are the values.
/// There are many examples in the JSON where contiguous JSON objects actually relate to a single conversation (e.g. cx2U0vz_0 .. cx2UOvz_5 ...).
/// This function will combine those into a single conversation (e.g. cx2UOvz).
/// 
/// If the environment variable SEED_ERRORS is set, this function will use rng to introduce a few errors to random messages.
fn parse_convos_into_hashmap<'de, D>(deserializer: D) -> Result<HashMap<String, Vec<Message>>, D::Error>
where
    D: Deserializer<'de>,
{
    //Visit each conversation in turn and build a hash map where the id is the key and the array of messages are the value
    struct ConversationVisitor(PhantomData<fn() -> HashMap<String, Vec<Message>>>);

    impl<'de> Visitor<'de> for ConversationVisitor
    {
        type Value = HashMap<String, Vec<Message>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("An array of objects with keys 'id' and 'conversations'. \n'id' should be a string, and 'conversations' an array of messages each of which has a 'from' and a 'value' key.")
        }

        //Visit each item in the sequence
        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: SeqAccess<'de>, {
            let mut map: HashMap<String, Vec<Message>> = HashMap::with_capacity(10000);

            let seed_errors = env::var("SEED_ERRORS").is_ok();
            let mut rng = rand::thread_rng();

            //Loops through each JSON object, using serde to parse it into a Conversation struct
            while let Some(conversation) = seq.next_element::<Conversation>()? {
                let id: String = conversation.id;
                let mut conversations: Vec<Message> = conversation.conversations;
                
                if seed_errors {
                    if rng.gen_range(0.0..1.0) < 0.01 && conversations.len() > 0 { //Introduce some randomness to check error checking is working correctly
                        conversations = conversations.iter().skip(rng.gen_range(0..conversations.len())).map(|message| Message {role: message.role.clone(), content: message.content.clone() + "error"}).collect();
                    }
                }

                //Get the actual conversation id, discarding the number after the underscore
                let general_id = id.split('_').next().unwrap();
                if map.contains_key(general_id) && conversations.len() != 0 { //We have previously inserted data which matches this id
                    let preexisting_conversation = map.get_mut(general_id).unwrap();
                    //Handle case where the final message of one conversation array is duplicated as the first message of the next conversation array by skipping the duplicated message
                    if preexisting_conversation.len() > 0 && preexisting_conversation.last().expect(&format!("Preexisting messages for {} shouldn't be empty", general_id)) == conversations.first().expect(&format!("Messages for {} shouldn't be empty", general_id)) {
                        preexisting_conversation.extend(conversations.into_iter().skip(1));
                    } else {
                        preexisting_conversation.extend(conversations);
                    }
                } else { //No matching data exists already, so we're good to insert it directly
                    map.insert(general_id.to_string(), conversations);
                }
            }
            Ok(map)
        }
    }   

    let visitor = ConversationVisitor(PhantomData);
    deserializer.deserialize_seq(visitor)
}

#[derive(Deserialize)]
struct Conversation {
    id: String,
    conversations: Vec<Message>
}

/// Struct to tie a single message to the conversation id it came from.
/// This is useful in cases where the same exact message appears multiple times in the input JSON, so
/// if our test gets back a different response to the one it expected it can use the id value to check that
/// the returned value is indeed valid.
pub struct Response {
    pub id: String,
    pub response_message: Message
}

/// Struct for storing the conversations that have been parsed from the JSON file.
/// This struct is exposed to any external projects, which is easier to understand than passing a HashMap<String, Vec<Message>> around
/// and enforces the read only nature of the parsed data.
pub struct Conversations {
    conversations: HashMap<String, Vec<Message>>
}

/// The only way external projects can access conversations is via the get_conversation method, which returns an Option containing
/// an array of messages pertaining to the requested id, if it exists.
impl Conversations {
    pub fn get_conversation(&self, id: &String) -> Option<&Vec<Message>> {
        return self.conversations.get(id);
    }

    /// Helper function for the testing client to stream all conversations asynchronously.
    pub fn stream_conversations(&self) -> tokio_stream::Iter<std::collections::hash_map::Values<'_, std::string::String, Vec<Message>>>{
        return tokio_stream::iter(self.conversations.values());
    }
}

/// A function to read the JSON file into memory and parse it into a strongly typed Rust HashMap.
/// The keys are the conversation id.
/// The values are an array of each message in the conversation.
/// Since this Conversations struct will be read only from this point, and it is shared across threads, we wrap it in an atomic reference counter.
pub fn build_conversations_data_from_file() -> Result<Arc<Conversations>, Box<dyn std::error::Error>> {
    let json_file = File::open("data/ShareGPT_V3_unfiltered_cleaned_split.json")?;
    let file_reader = BufReader::with_capacity(681574400, json_file);

    let outer: Outer = serde_json::from_reader(file_reader)?;
    log::info!("Parsed {} conversations", outer.conversations.len());

    let conversations = Conversations { conversations: outer.conversations };

    return Ok(Arc::new(conversations));
}

/// Struct for storing the hashed responses that have been preprocessed.
/// This struct is exposed to external projects, which is easier to understand than passing a HashMap<String, Vec<Response>> around
/// and enforces the read only nature of the preprocessed data.
/// There are a number of conversations in the JSON file which start with exactly the same human message (often 'hi'), and so to handle this
/// a single message actually corresponds to a vector of possible valid responses. In most cases that vector only contains one valid response,
/// but in case there are multiple the HashedResponses implementation will handle choosing one at random and returning it for us.
pub struct HashedResponses {
    hashed_responses: HashMap<String, Vec<Response>>
}

/// The only way external projects can access hashed responses is via the get_response_for_requests method, which returns an Option containing
/// a valid response for the chat completion request, if one exists. If there are multiple valid responses for the given request it will select one
/// at random and return it.
impl HashedResponses {
    pub fn get_response_for_request(&self, request: &ChatCompletionRequest) -> Option<&Response> {
        let request_hash = request.hash();
        let valid_responses = self.hashed_responses.get(&request_hash);
        let rand_selected_response = valid_responses.map(|valid_responses: &Vec<Response>| valid_responses.choose(&mut rand::thread_rng()).unwrap());
        return rand_selected_response;
    }
}

/// A function to preprocess the JSON data into a format that will allow for O(1) lookups for any valid request.
/// Since we know ahead of time every possible input and output, using a hashmap is an obvious optimisation.
/// The result of this function is a HashMap where the values contain every possible Assistant response,
/// and the key for each response is the combined string of every message prior to that response in associated the conversation.
/// As such, if this is the first response the Assistant has given in a particular conversation then the key will simply be the
/// contents of the one message the User sent. On the other hand, if this is the 50th Assistant response in a long conversation chain,
/// the key will be the concatenated values of all messages passes between the User and the Assistant prior.
/// This initially seems slightly wasteful but is actually imperative, since LLMs make heavy use of context. If two different users were to ask
/// chatgpt 'what did I just say' you would expect them to each get very different responses, so each Assistant response is informed by every
/// message that came before it.
pub fn preprocess_hashed_responses(conversations_data: &Arc<Conversations>) -> Arc<HashedResponses> {
    let mut hashmap: HashMap<String, Vec<Response>> = HashMap::with_capacity(500000);
    for (id, conversation) in conversations_data.conversations.iter() {
        let mut string_until_now = String::from("");
        for message in conversation {
            if message.role == Role::Assistant {
                match hashmap.get_mut(&string_until_now) {
                    Some(responses) => responses.push(Response { id: id.clone(), response_message: message.clone() }),
                    None => {
                        hashmap.insert(string_until_now.clone(), vec![Response { id: id.clone(), response_message: message.clone()}]);
                    }
                };
            }
            string_until_now.push_str(&message.content);
        };
    };

    log::info!("Built {} hashed responses", hashmap.len());

    let hashed_responses = HashedResponses { hashed_responses: hashmap };

    return Arc::new(hashed_responses);
}