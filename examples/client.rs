use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tipsy::{Endpoint, IntoIpcPath, ServerId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SocketCommand {
    GetVersion,
    GetClash,
    StartClash(StartBody),
    StopClash,
    StopService,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StartBody {
    pub core_type: Option<String>,
    pub bin_path: String,
    pub config_dir: String,
    pub config_file: String,
    pub log_file: String,
    pub use_local_socket: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JsonResponse {
    pub code: u64,
    pub msg: String,
    pub data: Option<String>,
}

#[tokio::main]
#[allow(deprecated)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = ServerId::new("verge-server").parent_folder(std::env::temp_dir());
    println!("Server path: {:?}", path.clone().into_ipc_path()?);
    let client = Endpoint::connect(path).await?;

    let mut count = 0;
    let mut reader = BufReader::new(client);
    while count < 1 {
        let home_dir = std::env::home_dir().unwrap();
        let config_dir = home_dir.join(".local/share/io.github.oomeow.clash-verge-self");
        let config_file = config_dir.join("clash-verge.yaml");
        let log_file = config_dir.join("logs/service/aaaaaaaa.log");
        let param = SocketCommand::StartClash(StartBody {
            core_type: Some("verge-mihomo-alpha".to_string()),
            bin_path: "/usr/bin/verge-mihomo-alpha".to_string(),
            config_dir: config_dir.to_string_lossy().to_string(),
            config_file: config_file.to_string_lossy().to_string(),
            log_file: log_file.to_string_lossy().to_string(),
            use_local_socket: false,
        });
        // let param = SocketCommand::GetClash;
        let param = SocketCommand::StopClash;
        // let param = SocketCommand::GetVersion;
        // let param = SocketCommand::StopService;
        let mut request_params = serde_json::to_string(&param).unwrap();
        request_params.push('\n');
        reader
            .write_all(request_params.as_bytes())
            .await
            .expect("Unable to write message to client");

        let mut buf = String::new();
        reader.read_line(&mut buf).await?;
        println!("RECV: {:?}", buf);
        let json: JsonResponse = serde_json::from_str(&buf).unwrap();
        println!("JSON: {:?}", json);

        count += 1;
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    Ok(())
}
