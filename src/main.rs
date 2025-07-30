mod crypto;
mod log_config;
mod service;

use log_config::LogConfig;

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    let _ = LogConfig::global().lock().init(None);
    service::main()
}

#[cfg(not(windows))]
fn main() {
    let _ = LogConfig::global().lock().init(None);
    service::main();
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use anyhow::{Ok, Result};
    use tipsy::{Connection, Endpoint, IntoIpcPath, ServerId};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use crate::{
        crypto::{decrypt_socket_data, encrypt_socket_data, load_keys},
        log_config::LogConfig,
        service::{
            ClashStatus, SERVER_ID,
            data::{JsonResponse, SocketCommand, StartBody},
            run_service,
        },
    };

    #[tokio::test]
    async fn test_start_server() -> Result<()> {
        let _ = LogConfig::global().lock().init(None);
        run_service().await?;
        Ok(())
    }

    async fn connect_client() -> Result<Connection> {
        let path = ServerId::new(SERVER_ID).parent_folder(std::env::temp_dir());
        println!("Server path: {:?}", path.clone().into_ipc_path()?);
        let client = Endpoint::connect(path).await?;
        Ok(client)
    }

    async fn send(reader: &mut BufReader<Connection>, cmd: SocketCommand) -> Result<String> {
        let (private_key, public_key) = load_keys()?;

        let request_params = serde_json::to_string(&cmd)?;
        let combined = encrypt_socket_data(&public_key, &request_params)?;
        reader.write_all(combined.as_bytes()).await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        response = decrypt_socket_data(&private_key, &response).unwrap();

        Ok(response)
    }

    #[tokio::test]
    async fn test_get_version() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let response = send(&mut reader, SocketCommand::GetVersion).await?;
        let json: JsonResponse<HashMap<String, String>> = serde_json::from_str(&response)?;
        println!("{json:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_start_core() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let home_dir = std::env::home_dir().unwrap();
        let config_dir = home_dir.join(".local/share/io.github.oomeow.clash-verge-self");
        let config_file = config_dir.join("clash-verge.yaml");
        let log_file = config_dir.join("logs/service/aaaaaaaa.log");
        let param = SocketCommand::StartClash(StartBody {
            core_type: Some("verge-mihomo-alpha".to_string()),
            #[cfg(unix)]
            socket_path: Some("/tmp/verge-mihomo-test.sock".to_string()),
            #[cfg(windows)]
            socket_path: Some(r"\\.\pipe\verge-mihomo-test".to_string()),
            bin_path: "/usr/bin/verge-mihomo-alpha".to_string(),
            config_dir: config_dir.to_string_lossy().to_string(),
            config_file: config_file.to_string_lossy().to_string(),
            log_file: log_file.to_string_lossy().to_string(),
        });

        let response = send(&mut reader, param).await?;
        let json: JsonResponse<()> = serde_json::from_str(&response)?;
        println!("{json:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_clash() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let response = send(&mut reader, SocketCommand::GetClash).await?;
        let json: JsonResponse<ClashStatus> = serde_json::from_str(&response)?;
        println!("{json:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_logs() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let response = send(&mut reader, SocketCommand::GetLogs).await?;
        println!("{}", response);
        let json: JsonResponse<Vec<String>> = serde_json::from_str(&response)?;
        if let Some(logs) = json.data {
            for log in logs {
                println!("{log}");
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_stop_clash() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let response = send(&mut reader, SocketCommand::StopClash).await?;
        let json: JsonResponse<()> = serde_json::from_str(&response)?;
        println!("{json:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_stop_service() -> Result<()> {
        let client = connect_client().await?;
        let mut reader = BufReader::new(client);
        let response = send(&mut reader, SocketCommand::StopService).await?;
        let json: JsonResponse<()> = serde_json::from_str(&response)?;
        println!("{json:?}");
        Ok(())
    }
}
