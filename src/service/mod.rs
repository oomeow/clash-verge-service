pub mod data;
mod handle;
mod logger;
#[cfg(test)]
pub use handle::ClashStatus;

use crate::crypto::decrypt_socket_data;
use crate::crypto::encrypt_socket_data;
use crate::crypto::generate_rsa_keys;
use crate::crypto::load_keys;
use data::JsonResponse;
use data::SocketCommand;
use futures_util::StreamExt;
use handle::get_clash;
use handle::get_logs;
use handle::get_version;
use handle::start_clash;
use handle::stop_clash;
use rsa::RsaPrivateKey;
use rsa::RsaPublicKey;

use tipsy::Connection;
use tipsy::Endpoint;
use tipsy::OnConflict;
use tipsy::SecurityAttributes;
use tipsy::ServerId;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::sync::watch::channel;
#[cfg(windows)]
use windows_service::{
    Result, define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

#[cfg(windows)]
pub const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
pub const SERVICE_NAME: &str = "clash_verge_service";
pub const DEFAULT_SERVER_ID: &str = "verge-service-server";

macro_rules! wrap_response {
    ($expr: expr) => {
        match $expr {
            Ok(data) => serde_json::to_string(&JsonResponse {
                code: 0,
                msg: "ok".into(),
                data: Some(data),
            }),
            Err(err) => serde_json::to_string(&JsonResponse {
                code: 400,
                msg: format!("{err}"),
                data: Option::<()>::None,
            }),
        }
    };
}

/// The Service
pub async fn run_service(server_id: Option<String>) -> anyhow::Result<()> {
    // 开启服务 设置服务状态
    #[cfg(windows)]
    let status_handle = service_control_handler::register(
        SERVICE_NAME,
        move |event| -> ServiceControlHandlerResult {
            match event {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => std::process::exit(0),
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        },
    )?;
    #[cfg(windows)]
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    let server_id = server_id.unwrap_or(DEFAULT_SERVER_ID.to_string());
    let instant = std::time::Instant::now();
    let (private_key, public_key) = match load_keys() {
        Ok(keys) => keys,
        Err(_) => {
            log::error!("failed to load keys form file, starting regenerate keys and save keys");
            generate_rsa_keys()?
        }
    };
    log::debug!("load rsa keys took {:?}", instant.elapsed());

    let path = ServerId::new(server_id).parent_folder(std::env::temp_dir());
    let security_attributes = SecurityAttributes::allow_everyone_connect()?;
    let mut incoming = Endpoint::new(path, OnConflict::Overwrite)?
        .security_attributes(security_attributes)
        .incoming()?;

    let (shutdown_tx, mut shutdown_rx) = channel(());
    loop {
        tokio::select! {
            Some(result) = incoming.next() => {
                match result {
                    Ok(stream) => {
                        let reader = BufReader::new(stream);
                        spawn_read_task(private_key.clone(),public_key.clone(), reader, shutdown_tx.clone()).await;
                    }
                    _ => unreachable!("ideally")
                }
            }
            _ = shutdown_rx.changed() => {
                let _ = stop_service();
                log::info!("Shutdown Service");
                break;
            }
        }
    }

    Ok(())
}

async fn spawn_read_task(
    private_key: RsaPrivateKey,
    public_key: RsaPublicKey,
    mut reader: BufReader<Connection>,
    shutdown_tx: tokio::sync::watch::Sender<()>,
) {
    tokio::spawn(async move {
        loop {
            let mut msg = String::new();
            match reader.read_line(&mut msg).await {
                Ok(size) if size > 0 => match decrypt_socket_data(&private_key.clone(), &msg) {
                    Ok(req_data) => match serde_json::from_str::<SocketCommand>(&req_data) {
                        Ok(cmd) => {
                            if handle_socket_command(&public_key.clone(), &mut reader, cmd.clone())
                                .await
                                .is_err()
                            {
                                log::error!("Error handling socket command");
                            }
                            if let SocketCommand::StopService = cmd {
                                let _ = reader.shutdown().await;
                                let _ = shutdown_tx.send(());
                                break;
                            }
                        }
                        Err(err) => {
                            log::error!("Error parsing socket command: {err}");
                        }
                    },
                    Err(err) => {
                        log::error!("Error decrypting socket data: {err}");
                        let err_res = Result::<(), anyhow::Error>::Err(err);
                        let response = wrap_response!(err_res).unwrap();
                        let combined = encrypt_socket_data(&public_key, &response).unwrap();
                        reader.write_all(combined.as_bytes()).await.unwrap();
                    }
                },
                Ok(_) => {
                    log::debug!("empty line, the socket is closed");
                    break;
                }
                Err(err) => {
                    log::error!("read error: {err}");
                    break;
                }
            }
        }
        log::info!("Connection closed");
    });
}

async fn handle_socket_command(
    public_key: &RsaPublicKey,
    reader: &mut BufReader<Connection>,
    cmd: SocketCommand,
) -> anyhow::Result<()> {
    log::info!("Handling socket command: {cmd:?}");
    let response = match cmd {
        SocketCommand::GetVersion => wrap_response!(get_version())?,
        SocketCommand::GetClash => wrap_response!(get_clash())?,
        SocketCommand::GetLogs => wrap_response!(get_logs())?,
        SocketCommand::StartClash(body) => wrap_response!(start_clash(body))?,
        SocketCommand::StopClash => {
            #[cfg(unix)]
            let socket_path = {
                use crate::service::handle::ClashStatus;

                let clash_status = ClashStatus::global().lock().clone();
                clash_status.info.and_then(|i| i.socket_path)
            };
            let res = wrap_response!(stop_clash())?;
            #[cfg(unix)]
            {
                if let Some(socket_path) = socket_path {
                    log::info!("delete socket path");
                    let path = std::path::Path::new(&socket_path);
                    if path.exists() {
                        std::fs::remove_file(path)?;
                    }
                }
            }
            res
        }
        SocketCommand::StopService => wrap_response!(anyhow::Result::<()>::Ok(()))?,
    };
    let combined = encrypt_socket_data(public_key, &response)?;
    reader.write_all(combined.as_bytes()).await?;
    Ok(())
}

/// 停止服务
#[cfg(windows)]
fn stop_service() -> Result<()> {
    let status_handle =
        service_control_handler::register(SERVICE_NAME, |_| ServiceControlHandlerResult::NoError)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

#[cfg(not(windows))]
fn stop_service() -> anyhow::Result<()> {
    // systemctl stop clash_verge_service
    std::process::Command::new("systemctl")
        .arg("stop")
        .arg(SERVICE_NAME)
        .output()
        .expect("failed to execute process");
    Ok(())
}

// #[cfg(windows)]
// define_windows_service!(ffi_service_main, my_service_main);

// #[cfg(windows)]
// pub fn my_service_main(arguments: Vec<std::ffi::OsString>) {
//     if let Ok(rt) = Runtime::new() {
//         let args = arguments
//             .iter()
//             .map(|arg| arg.to_string_lossy().to_string())
//             .collect::<Vec<String>>();
//         log::info!("arguments: {:?}", args);
//         let server_id = if args.len() == 2 {
//             Some(args[1].clone())
//         } else {
//             None
//         };
//         rt.block_on(async {
//             let _ = run_service(server_id).await;
//         });
//     }
// }

// pub fn main() -> anyhow::Result<()> {
//     #[cfg(not(windows))]
//     if let Ok(rt) = Runtime::new() {
//         let (_, server_id) = crate::utils::parse_args()?;
//         rt.block_on(async {
//             let _ = run_service(server_id).await;
//         });
//     }
//     #[cfg(windows)]
//     service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
//     Ok(())
// }
