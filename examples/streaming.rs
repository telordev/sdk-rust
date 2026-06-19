//! Streaming a message and printing text deltas live.
//!
//! Run with: `SIMSE_API_KEY=sk_... cargo run --example streaming`

use std::io::Write;

use futures::StreamExt;
use simse::types::{ContentDelta, InputMessage, MessageStreamEvent};
use simse::{Client, MessageCreateParams};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let mut stream = client
        .messages()
        .stream(
            MessageCreateParams::builder("zoysia", 1024)
                .message(InputMessage::user("Write a short haiku about Rust."))
                .build(),
        )
        .await?;

    while let Some(event) = stream.next().await {
        match event? {
            MessageStreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => {
                print!("{text}");
                let _ = std::io::stdout().flush();
            }
            MessageStreamEvent::MessageDelta { usage, .. } => {
                eprintln!("\n[output tokens: {}]", usage.output_tokens);
            }
            MessageStreamEvent::MessageStop => println!(),
            _ => {}
        }
    }

    Ok(())
}
