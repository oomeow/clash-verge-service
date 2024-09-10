mod data;
mod web;

use self::data::*;
use self::web::*;
use tokio::runtime::Runtime;
use warp::Filter;

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
const LISTEN_PORT: u16 = 33211;

macro_rules! wrap_response {
    ($expr: expr) => {
        match $expr {
            Ok(data) => warp::reply::json(&JsonResponse {
                code: 0,
                msg: "ok".into(),
                data: Some(data),
            }),
            Err(err) => warp::reply::json(&JsonResponse {
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

    let api_get_version = warp::get()
        .and(warp::path("version"))
        .map(move || wrap_response!(get_version()));

    let api_start_clash = warp::post()
        .and(warp::path("clash"))
        .and(warp::body::json())
        .map(move |body: StartBody| wrap_response!(start_clash(body)));

    let api_get_clash = warp::get()
        .and(warp::path("clash"))
        .map(move || wrap_response!(get_clash()));

    let api_stop_clash = warp::delete()
        .and(warp::path("clash"))
        .map(move || wrap_response!(stop_clash()));

    let api_update_log_level = warp::put()
        .and(warp::path("loglevel"))
        .and(warp::body::json())
        .map(|body: LogLevelBody| wrap_response!(update_log_level(body)));

    let api_stop_service = warp::delete()
        .and(warp::path("service"))
        .map(|| wrap_response!(stop_service()));

    warp::serve(
        api_get_version
            .or(api_start_clash)
            .or(api_get_clash)
            .or(api_stop_clash)
            .or(api_update_log_level)
            .or(api_stop_service),
    )
    .run(([127, 0, 0, 1], LISTEN_PORT))
    .await;

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
