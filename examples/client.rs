use std::time::Instant;

use clash_verge_self_service::model::{ServiceVersionInfo, SocketCommand};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server_id = "hello-secured-ipc-dev";
    let mut client = clash_verge_self_service::Client::connect(server_id, Some(clash_verge_self_service::PSK)).await?;
    let now = Instant::now();
    for _ in 0..=2000 {
        println!("------------------------");
        let msg = client.send::<ServiceVersionInfo>(SocketCommand::GetVersion).await?;
        println!("response: {:?}", msg);
    }
    println!("took: {}ms", now.elapsed().as_millis());
    Ok(())
}
