//! List the hosted models and retrieve one by id.
//!
//! Run with: `SIMSE_API_KEY=sk_... cargo run --example list_models`

use simse::Client;

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let models = client.models().list().await?;
    println!("{} models (has_more={}):", models.data.len(), models.has_more);
    for m in &models.data {
        println!(
            "  {:8} {:30} max_out={:?}",
            m.id, m.display_name, m.max_tokens
        );
    }

    // Retrieve one by id.
    if let Some(first) = models.data.first() {
        let one = client.models().retrieve(&first.id).await?;
        println!("\nretrieved: {} ({})", one.id, one.display_name);
    }

    Ok(())
}
