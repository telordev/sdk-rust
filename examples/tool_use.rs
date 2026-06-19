//! Tool use: define a tool, get a `tool_use` block, send the result back.
//!
//! Run with: `SIMSE_API_KEY=sk_... cargo run --example tool_use`

use serde_json::json;
use simse::types::{ContentBlock, InputMessage, Role, Tool};
use simse::{Client, MessageCreateParams};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let weather = Tool::new(
        "get_weather",
        json!({
            "type": "object",
            "properties": { "city": { "type": "string", "description": "City name" } },
            "required": ["city"]
        }),
    )
    .with_description("Get the current weather for a city.");

    let question = "What's the weather in San Francisco?";

    let first = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 1024)
                .tool(weather.clone())
                .message(InputMessage::user(question))
                .build(),
        )
        .await?;

    // Find the tool call.
    let mut tool_use_id = None;
    for block in &first.content {
        if let ContentBlock::ToolUse { name, input, id } = block {
            println!("model wants to call {name} with {input}");
            tool_use_id = Some(id.clone());
        }
    }

    let Some(id) = tool_use_id else {
        println!("no tool call — model answered directly:\n{}", first.text());
        return Ok(());
    };

    // Execute the tool (here: a stub) and return its result.
    let tool_output = "Sunny, 72°F";

    let follow_up = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 1024)
                .tool(weather)
                .message(InputMessage::user(question))
                .message(InputMessage::blocks(Role::Assistant, first.content.clone()))
                .message(InputMessage::blocks(
                    Role::User,
                    vec![ContentBlock::tool_result(id, tool_output)],
                ))
                .build(),
        )
        .await?;

    println!("final answer: {}", follow_up.text());
    Ok(())
}
