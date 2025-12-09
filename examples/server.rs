#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server_id = "hello-secured-ipc-dev";
    clash_verge_self_service::Server::run(server_id, Some(clash_verge_self_service::PSK)).await?;
    Ok(())
}
