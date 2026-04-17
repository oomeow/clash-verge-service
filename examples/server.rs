use anyhow::Result;

const SERVER_ID: &str = "hello-secured-ipc-dev";

#[tokio::main]
async fn main() -> Result<()> {
    println!("starting IPC server: {SERVER_ID}");
    clash_verge_self_service::Server::run(SERVER_ID).await?;
    Ok(())
}
