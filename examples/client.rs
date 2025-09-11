use tipsy::{Endpoint, ServerId};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = "hello-secured-ipc-dev";
    let client = Endpoint::connect(ServerId::new(path).parent_folder(std::env::temp_dir()))
        .await
        .expect("Failed to connect client.");

    let psk = b"asdoekxfsdedadxjasd";
    let mut secured = clash_verge_service::service::SecureChannel::handshake_client(client, Some(psk)).await?;
    for _ in 0..=20 {
        let plaintext = serde_json::to_string(&clash_verge_service::service::data::SocketCommand::GetVersion)?;
        secured.send(plaintext.as_bytes()).await?;
        let response = secured.recv().await?;
        let msg = String::from_utf8(response)?;
        println!("response: {msg}");
    }
    Ok(())
}
