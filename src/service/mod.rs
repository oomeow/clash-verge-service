mod data;
mod handle;
mod logger;

use data::JsonResponse;
use data::SocketCommand;
use futures_util::StreamExt;
use handle::get_clash;
use handle::get_logs;
use handle::get_version;
use handle::start_clash;
use handle::stop_clash;
use handle::ClashStatus;
use tipsy::Connection;
use tipsy::Endpoint;
use tipsy::OnConflict;
use tipsy::SecurityAttributes;
use tipsy::ServerId;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::runtime::Runtime;
use tokio::sync::watch::channel;

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

#[cfg(unix)]
pub const SOCKET_PATH: &str = "/tmp/verge-mihomo.sock";
#[cfg(windows)]
pub const SOCKET_PATH: &str = r#"\\.\pipe\verge-mihomo"#;

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
                        spawn_read_task(reader, shutdown_tx.clone()).await;
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
    mut reader: BufReader<Connection>,
    shutdown_tx: tokio::sync::watch::Sender<()>,
) {
    tokio::spawn(async move {
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf).await {
                Ok(size) if size > 0 => match serde_json::from_str::<SocketCommand>(&buf) {
                    Ok(cmd) => {
                        if handle_socket_command(&mut reader, cmd.clone())
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

async fn handle_socket_command(
    reader: &mut BufReader<Connection>,
    cmd: SocketCommand,
) -> anyhow::Result<()> {
    log::info!("Handling socket command: {:?}", cmd);
    let response = match cmd {
        SocketCommand::GetVersion => wrap_response!(get_version())?,
        SocketCommand::GetClash => wrap_response!(get_clash())?,
        SocketCommand::GetLogs => wrap_response!(get_logs())?,
        SocketCommand::StartClash(body) => wrap_response!(start_clash(body))?,
        SocketCommand::StopClash => {
            #[cfg(unix)]
            let clash_info = {
                let clash_status = ClashStatus::global().lock();
                log::info!("clash status {:?}", clash_status);
                clash_status.info.clone()
            };
            let res = wrap_response!(stop_clash())?;
            #[cfg(unix)]
            {
                if let Some(clash_info) = clash_info {
                    if clash_info.use_local_socket {
                        log::info!("delete socket path");
                        let path = std::path::Path::new(SOCKET_PATH);
                        if path.exists() {
                            std::fs::remove_file(path)?;
                        }
                    }
                }
            }
            res
        }
        SocketCommand::StopService => wrap_response!(anyhow::Result::<()>::Ok(()))?,
    };
    let data = format!("{}\n", response);
    reader.write_all(data.as_bytes()).await?;
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
