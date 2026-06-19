//! Sessions: create a session, prompt it (buffered + streaming), then delete.
//!
//! Run with: `SIMSE_API_KEY=sk_... cargo run --example session`

use futures::StreamExt;
use simse::params::SessionCreateParams;
use simse::types::SessionStreamEvent;
use simse::Client;

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let session = client
        .sessions()
        .create(SessionCreateParams::new().title("SDK demo session"))
        .await?;
    println!("created session {} (status={})", session.id, session.status);

    // Buffered prompt.
    let result = client
        .sessions()
        .prompt(&session.id, "In one sentence, what is Rust's borrow checker?")
        .await?;
    println!("\nbuffered reply:\n{}", result.message.content);
    println!(
        "usage: {} input + {} output = {} total tokens",
        result.usage.input_tokens,
        result.usage.output_tokens,
        result.usage.total_tokens()
    );

    // Streaming prompt.
    println!("\nstreaming reply:");
    let mut stream = client
        .sessions()
        .stream(&session.id, "Now give one practical tip.")
        .await?;
    while let Some(event) = stream.next().await {
        match event? {
            SessionStreamEvent::Delta { delta } => print!("{delta}"),
            SessionStreamEvent::ToolCall { name, .. } => eprintln!("\n[tool: {name}]"),
            SessionStreamEvent::Done { status, .. } => println!("\n[done: {status}]"),
            _ => {}
        }
    }

    // List + clean up.
    let sessions = client.sessions().list().await?;
    println!("\nyou have {} session(s)", sessions.sessions.len());

    client.sessions().delete(&session.id).await?;
    println!("deleted {}", session.id);

    Ok(())
}
