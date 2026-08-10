use serde::{Deserialize, Serialize};

pub const DEFAULT_INSTRUCTION: &str = "Post something in the IRC channel.";

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 1],
    temperature: f32,
    max_tokens: u32,
    n: u32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessageOut,
}

#[derive(Deserialize)]
struct ChatCompletionMessageOut {
    content: String,
}

pub async fn generate_quote(
    client: &reqwest::Client,
    endpoint: &str,
    instruction: &str,
    temperature: f32,
    max_tokens: u32,
) -> Result<String, reqwest::Error> {
    let url = format!("{}/v1/chat/completions", endpoint.trim_end_matches('/'));
    let body = ChatCompletionRequest {
        model: "local",
        messages: [ChatMessage {
            role: "user",
            content: instruction,
        }],
        temperature,
        max_tokens,
        n: 1,
    };

    let response = client
        .post(url)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json::<ChatCompletionResponse>()
        .await?;

    Ok(response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .unwrap_or_default())
}
