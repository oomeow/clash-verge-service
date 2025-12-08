#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = "hello-secured-ipc-dev";
    clash_verge_self_service::service::run_service(
        Some(path.to_string()),
        Some(clash_verge_self_service::service::PSK),
    )
    .await?;
    Ok(())
}
