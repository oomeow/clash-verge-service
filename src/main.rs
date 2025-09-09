mod crypto;
mod install;
mod log_config;
mod service;
mod uninstall;
mod utils;

use std::path::PathBuf;

use log_config::LogConfig;

use clap::{Parser, Subcommand};
#[cfg(windows)]
use windows_service::{define_windows_service, service_dispatcher};

#[derive(Parser)]
#[command(version, about = "install, uninstall or run Clash Verge Service", long_about = None)]
struct Cli {
    #[arg(short, long, help = "run service by using this server id for IPC")]
    server_id: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "install Clash Verge Service")]
    Install {
        #[arg(short, long, help = "log directory")]
        log_dir: Option<PathBuf>,

        #[arg(short, long, help = "server id of IPC")]
        server_id: Option<String>,
    },
    #[command(about = "uninstall Clash Verge Service")]
    Uninstall {
        #[arg(short, long, help = "log directory")]
        log_dir: Option<PathBuf>,
    },
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
pub fn my_service_main(arguments: Vec<std::ffi::OsString>) {
    if let Ok(rt) = tokio::runtime::Runtime::new() {
        let args = arguments
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<String>>();
        log::info!("arguments: {:?}", args);
        let server_id = if args.len() == 2 {
            Some(args[1].clone())
        } else {
            None
        };
        rt.block_on(async {
            let _ = crate::service::run_service(server_id).await;
        });
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Install { log_dir, server_id }) => {
            LogConfig::global().lock().init(log_dir)?;
            crate::install::process(server_id)?;
        }
        Some(Commands::Uninstall { log_dir }) => {
            LogConfig::global().lock().init(log_dir)?;
            crate::uninstall::process()?;
        }
        None => {
            LogConfig::global().lock().init(None)?;
            #[cfg(not(windows))]
            {
                let server_id = cli.server_id;
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async move {
                    let _ = crate::service::run_service(server_id).await;
                });
            }
            #[cfg(windows)]
            service_dispatcher::start(crate::service::SERVICE_NAME, ffi_service_main)?;
        }
    }

    Ok(())
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
            ClashStatus,
            data::{JsonResponse, SocketCommand, StartBody},
            run_service,
        },
    };

    #[tokio::test]
    async fn test_start_server() -> Result<()> {
        let _ = LogConfig::global().lock().init(None);
        run_service(Some(String::from("verge-test"))).await?;
        Ok(())
    }

    async fn connect_client() -> Result<Connection> {
        let server_id = "hello-verge-self";
        let path = ServerId::new(server_id).parent_folder(std::env::temp_dir());
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
        response = decrypt_socket_data(&private_key, &response)?;

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
