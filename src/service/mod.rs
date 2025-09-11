pub mod data;
mod handle;
mod logger;

use anyhow::{Result, anyhow};
use data::{JsonResponse, SocketCommand};
use futures_util::StreamExt;
#[cfg(test)]
pub use handle::ClashStatus;
use handle::{get_clash, get_logs, get_version, start_clash, stop_clash};
use rsa::{RsaPrivateKey, RsaPublicKey};
use tipsy::{Connection, Endpoint, OnConflict, SecurityAttributes, ServerId};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::watch::{Sender, channel},
};
#[cfg(windows)]
use windows_service::{
    service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType},
    service_control_handler::{self, ServiceControlHandlerResult},
};

use crate::crypto::{decrypt_socket_data, encrypt_socket_data, generate_rsa_keys, load_keys};

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
pub async fn run_service(server_id: Option<String>) -> Result<()> {
    // 开启服务 设置服务状态
    #[cfg(windows)]
    let status_handle = service_control_handler::register(SERVICE_NAME, move |event| -> ServiceControlHandlerResult {
        match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => std::process::exit(0),
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;
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
    let incoming = Endpoint::new(path, OnConflict::Overwrite)?
        .security_attributes(security_attributes)
        .incoming()?;
    futures_util::pin_mut!(incoming);

    let (shutdown_tx, mut shutdown_rx) = channel(());

    tokio::select! {
         _ = async {
            while let Some(result) = incoming.next().await {
                match result {
                    Ok(stream) => {
                        let reader = BufReader::new(stream);
                        spawn_read_task(private_key.clone(),public_key.clone(), reader, shutdown_tx.clone()).await;
                    }
                    _ => unreachable!("ideally")
                }
            }
        } => { }
        _ = shutdown_rx.changed() => {
            let _ = stop_service();
            log::info!("Shutdown Service");
        }
    }

    Ok(())
}

async fn spawn_read_task(
    private_key: RsaPrivateKey,
    public_key: RsaPublicKey,
    mut reader: BufReader<Connection>,
    shutdown_tx: Sender<()>,
) {
    tokio::spawn(async move {
        let res: Result<()> = async {
            loop {
                let mut msg = String::new();
                let size = reader.read_line(&mut msg).await.map_err(|err| {
                    log::error!("Error reading from socket: {err}");
                    anyhow!("Error reading from socket: {err}")
                })?;
                if size > 0 {
                    let req_data = decrypt_socket_data(&private_key.clone(), &msg).map_err(|err| {
                        log::error!("Error decrypting socket data: {err}");
                        anyhow!("Error decrypting socket data: {err}")
                    })?;

                    let cmd = serde_json::from_str::<SocketCommand>(&req_data).map_err(|err| {
                        log::error!("Error parsing socket command: {err}");
                        anyhow!("Error parsing socket command: {err}")
                    })?;

                    handle_socket_command(&public_key, &mut reader, cmd.clone())
                        .await
                        .map_err(|err| {
                            log::error!("Error handling socket command: {err}");
                            anyhow!("Error handling socket command: {err}")
                        })?;
                    if let SocketCommand::StopService = cmd {
                        reader.shutdown().await?;
                        let _ = shutdown_tx.send(());
                        break Ok(());
                    }
                } else {
                    log::debug!("empty line, the socket is closed");
                    break Ok(());
                }
            }
        }
        .await;

        if res.is_err() {
            log::info!("send error response to back");
            let response = wrap_response!(res)?;
            let combined = encrypt_socket_data(&public_key, &response)?;
            reader.write_all(combined.as_bytes()).await?;
        }

        log::info!("Connection closed");

        Result::<()>::Ok(())
    });
}

/// handle socket command and write response message
async fn handle_socket_command(
    public_key: &RsaPublicKey,
    reader: &mut BufReader<Connection>,
    cmd: SocketCommand,
) -> Result<()> {
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
        SocketCommand::StopService => wrap_response!(Result::<()>::Ok(()))?,
    };
    let combined = encrypt_socket_data(public_key, &response)?;
    reader.write_all(combined.as_bytes()).await?;
    Ok(())
}

/// 停止服务
#[cfg(windows)]
fn stop_service() -> Result<()> {
    let status_handle = service_control_handler::register(SERVICE_NAME, |_| ServiceControlHandlerResult::NoError)?;

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
fn stop_service() -> Result<()> {
    // systemctl stop clash_verge_service
    std::process::Command::new("systemctl")
        .arg("stop")
        .arg(SERVICE_NAME)
        .output()?;
    Ok(())
}
