use std::{path::PathBuf, time::Instant};

use tipsy::{Endpoint, ServerId};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = "test";
    let temp_dir = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    println!("temp dir: {}", temp_dir.display());
    let path = ServerId::new(path).parent_folder(temp_dir);
    let client = Endpoint::connect(path).await.expect("Failed to connect client.");

    let mut secured = clash_verge_self_service::service::SecureChannel::handshake_client(
        client,
        Some(clash_verge_self_service::service::PSK),
    )
    .await?;
    let now = Instant::now();
    for _ in 0..=2000 {
        println!("------------------------");
        let plaintext = serde_json::to_string(&clash_verge_self_service::service::data::SocketCommand::GetVersion)?;
        secured.send(plaintext.as_bytes()).await?;
        let response = secured.recv().await?;
        let msg = String::from_utf8(response)?;
        println!("response: {}", msg);
    }
    println!("took: {}ms", now.elapsed().as_millis());
    Ok(())
}
