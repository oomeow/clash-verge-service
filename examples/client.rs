use std::time::{Duration, Instant};

use anyhow::Result;
use clash_verge_self_service::{
    Client,
    model::{ClashRunInfo, ServiceVersionInfo, SocketCommand, StartBody},
};

const SERVER_ID: &str = "hello-secured-ipc-dev";

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::connect(SERVER_ID).await?;
    println!(
        "connected and claimed service, heartbeat: {:?}, lease ttl: {:?}",
        client.heartbeat_interval(),
        client.lease_ttl()
    );
    let client = Client::connect(SERVER_ID).await?;

    let result = demo_flow(&client).await;
    let release_result = client.release().await;

    if let Err(err) = release_result {
        eprintln!("release client failed: {err}");
    }

    result
}

async fn demo_flow(client: &Client) -> Result<()> {
    let now = Instant::now();
    for _ in 0..=2000 {
        get_version(client).await?;
    }
    println!("get version x2001 took: {}ms", now.elapsed().as_millis());

    start_core(client).await?;
    tokio::time::sleep(Duration::from_secs(5)).await;
    get_clash(client).await?;
    get_logs(client).await?;

    tokio::time::sleep(Duration::from_secs(5)).await;
    stop_core(client).await?;
    get_clash(client).await?;
    get_logs(client).await?;

    Ok(())
}

async fn start_core(client: &Client) -> Result<()> {
    let curr_dir = std::env::current_dir()?;
    let start_core = StartBody {
        core_type: Some("mihomo".into()),
        socket_path: Some("/tmp/mihomo-dev.sock".into()),
        bin_path: "/Applications/Clash Verge Self.app/Contents/MacOS/self-mihomo".into(),
        config_dir: curr_dir.join("examples/mihomo").to_str().unwrap().to_string(),
        config_file: curr_dir.join("examples/mihomo/test.yaml").to_str().unwrap().to_string(),
        log_file: curr_dir.join("examples/mihomo/test.log").to_str().unwrap().to_string(),
    };
    let msg = client.send::<()>(SocketCommand::StartClash(start_core)).await?;
    println!("start clash: {:?}", msg);
    Ok(())
}

async fn stop_core(client: &Client) -> Result<()> {
    let msg = client.send::<()>(SocketCommand::StopClash).await?;
    println!("stop clash: {:?}", msg);
    Ok(())
}

async fn get_clash(client: &Client) -> Result<()> {
    let msg = client.send::<ClashRunInfo>(SocketCommand::GetClash).await?;
    println!("get clash: {:#?}", msg);
    Ok(())
}

async fn get_logs(client: &Client) -> Result<()> {
    let msg = client.send::<Vec<String>>(SocketCommand::GetLogs).await?;
    println!("get logs: {:?}", msg);
    Ok(())
}

async fn get_version(client: &Client) -> Result<()> {
    let msg = client.send::<ServiceVersionInfo>(SocketCommand::GetVersion).await?;
    println!("get version: {:?}", msg);
    Ok(())
}
