mod data;
mod web;

use data::JsonResponse;
use data::SocketCommand;
use futures_util::StreamExt;
use tipsy::Connection;
use tipsy::Endpoint;
use tipsy::OnConflict;
use tipsy::ServerId;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::runtime::Runtime;
use web::get_clash;
use web::get_version;
use web::start_clash;
use web::stop_clash;

#[cfg(windows)]
use std::{ffi::OsString, time::Duration};
#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher, Result,
};

#[cfg(windows)]
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_NAME: &str = "clash_verge_service";

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
pub async fn run_service() -> anyhow::Result<()> {
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
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let path = ServerId::new("verge-server").parent_folder(std::env::temp_dir());
    let mut incoming = Endpoint::new(path, OnConflict::Overwrite)?.incoming()?;

    while let Some(result) = incoming.next().await {
        match result {
            Ok(stream) => {
                let mut reader = BufReader::new(stream);
                tokio::spawn(async move {
                    loop {
                        let mut buf = String::new();
                        match reader.read_line(&mut buf).await {
                            Ok(size) if size > 0 => match serde_json::from_str(&buf) {
                                Ok(cmd) => {
                                    if handle_socket_command(&mut reader, cmd).await.is_err() {
                                        log::error!("Error handling socket command");
                                    }
                                }
                                Err(err) => {
                                    log::error!("Error parsing socket command: {}", err);
                                }
                            },
                            Ok(_) => {
                                log::debug!("empty line, the socket is closed");
                                break;
                            }
                            Err(err) => {
                                log::error!("read error: {}", err);
                                break;
                            }
                        }
                    }
                    log::info!("Connection closed");
                });
            }
            _ => unreachable!("ideally"),
        }
    }

    Ok(())
}

async fn handle_socket_command(
    reader: &mut BufReader<Connection>,
    cmd: SocketCommand,
) -> anyhow::Result<()> {
    log::info!("Handling socket command: {:?}", cmd);
    let response = match cmd {
        SocketCommand::GetVersion => wrap_response!(get_version())?,
        SocketCommand::GetClash => wrap_response!(get_clash())?,
        SocketCommand::StartClash(body) => wrap_response!(start_clash(body))?,
        SocketCommand::StopClash => wrap_response!(stop_clash())?,
        SocketCommand::StopService => wrap_response!(stop_service())?,
    };
    let data = format!("{}\n", serde_json::to_string(&response)?);
    reader.write_all(data.as_bytes()).await?;
    Ok(())
}

// 停止服务
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
        wait_hint: Duration::default(),
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
/// Service Main function
#[cfg(windows)]
pub fn main() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

#[cfg(not(windows))]
pub fn main() {
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            let _ = run_service().await;
        });
    }
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
pub fn my_service_main(_arguments: Vec<OsString>) {
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            let _ = run_service().await;
        });
    }
}
