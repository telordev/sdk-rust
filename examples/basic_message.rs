//! Basic non-streaming message creation.
//!
//! Run with: `SIMSE_API_KEY=sk_... cargo run --example basic_message`

use simse::types::InputMessage;
use simse::{Client, MessageCreateParams};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let message = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 1024)
                .system("You are a concise, helpful assistant.")
                .message(InputMessage::user("What is the capital of France?"))
                .temperature(0.7)
                .build(),
        )
        .await?;

    println!("model: {}", message.model);
    println!("stop_reason: {:?}", message.stop_reason);
    println!("text: {}", message.text());
    println!(
        "usage: {} in / {} out",
        message.usage.input_tokens, message.usage.output_tokens
    );

    Ok(())
}
