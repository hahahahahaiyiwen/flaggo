use github_copilot_sdk::{Client, ClientMode, ClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args_os()
        .nth(1)
        .ok_or("runtime-probe requires an empty runtime-state directory")?;
    let client = Client::start(
        ClientOptions::default()
            .with_mode(ClientMode::Empty)
            .with_base_directory(base)
            .with_use_logged_in_user(false)
            .with_env_remove([
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "COPILOT_GITHUB_TOKEN",
                "COPILOT_SDK_AUTH_TOKEN",
                "OPENAI_API_KEY",
                "AZURE_OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
            ]),
    )
    .await?;
    client.stop().await?;
    println!("{{\"runtimeHandshake\":\"succeeded\",\"inferenceRequested\":false}}");
    Ok(())
}
