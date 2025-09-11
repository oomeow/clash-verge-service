#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = "hello-secured-ipc-dev";
    let psk = b"asdoekxfsdedadxjasd";
    clash_verge_service::service::run_service(Some(path.to_string()), Some(psk)).await?;
    Ok(())
}
