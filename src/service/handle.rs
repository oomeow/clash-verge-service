use std::{collections::VecDeque, ffi::OsString, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use process_supervisor::{ProcessEvent, ProcessLogConfig, ProcessSpec, ProcessSupervisor, RestartPolicy};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sysinfo::System;

use super::data::*;
use crate::service::logger::Logger;

const MAX_RESTART_CORE_COUNT: usize = 10;
const CORE_RESTART_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct ClashStatus {
    pub sidecar: ProcessSupervisor,
    pub info: Arc<Mutex<Option<StartBody>>>,
}

impl ClashStatus {
    pub fn global() -> &'static ClashStatus {
        static CLASHSTATUS: OnceCell<ClashStatus> = OnceCell::new();
        CLASHSTATUS.get_or_init(|| ClashStatus {
            sidecar: ProcessSupervisor::new(Some(Arc::new(Self::handle_sidecar_event))),
            info: Arc::new(Mutex::new(None)),
        })
    }

    fn handle_sidecar_event(event: ProcessEvent) {
        match event {
            ProcessEvent::Stdout { line, .. } | ProcessEvent::Stderr { line, .. } => {
                wrap_mihomo_log(&line);
                Logger::global().append_log(line);
            }
            ProcessEvent::RestartLimitReached { attempts, .. } => {
                log::error!("recover clash core count exceeded, skip");
                Logger::global().append_log(format!("mihomo core restart limit reached after {attempts} retries"));
            }
            ProcessEvent::Error { message, .. } => {
                Logger::global().append_log(message);
            }
            _ => {}
        }
    }
}

/// 获取服务进程的版本
pub fn get_version() -> Result<ServiceVersionInfo> {
    let version = env!("CARGO_PKG_VERSION");
    Ok(ServiceVersionInfo {
        version: version.into(),
        service: "Clash Verge Self Service".into(),
    })
}

async fn run_core(body: StartBody) -> Result<()> {
    let StartBody {
        core_type: _,
        socket_path,
        bin_path,
        config_dir,
        config_file,
        log_file,
    } = body;
    let mut spec = ProcessSpec::new("mihomo", bin_path);
    spec.args = vec![
        OsString::from("-d"),
        OsString::from(config_dir),
        OsString::from("-f"),
        OsString::from(config_file),
    ];
    if let Some(socket_path) = socket_path {
        spec.args.push(OsString::from(if cfg!(unix) {
            "-ext-ctl-unix"
        } else {
            "-ext-ctl-pipe"
        }));
        spec.args.push(OsString::from(socket_path));
    }

    spec.restart_policy = RestartPolicy {
        max_restarts: MAX_RESTART_CORE_COUNT,
        restart_delay: CORE_RESTART_INTERVAL,
    };
    spec.log_config = ProcessLogConfig {
        log_file: Some(log_file.into()),
        truncate_on_start: false,
    };
    ClashStatus::global().sidecar.start(spec).await?;

    Ok(())
}

/// wrap mihomo log to log::info, log::warn, log::error
fn wrap_mihomo_log(line: &str) {
    let re = Regex::new(r"level=(\w+)").unwrap();
    let level = re
        .captures(line)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
        .unwrap_or("info");
    match level {
        "error" => log::error!(target: "mihomo", "[mihomo] {line}"),
        "warning" => log::warn!(target: "mihomo", "[mihomo] {line}"),
        "info" => log::info!(target: "mihomo", "[mihomo] {line}"),
        "debug" => log::debug!(target: "mihomo", "[mihomo] {line}"),
        _ => log::debug!(target: "mihomo", "[mihomo] {line}"),
    }
}

/// 启动clash进程
pub async fn start_clash(body: StartBody) -> Result<()> {
    // stop the old clash bin
    log::debug!("start clash {body:?}");
    stop_clash().await?;
    {
        let clash_status = ClashStatus::global();
        *clash_status.info.lock() = Some(body.clone());
    }
    // get log file path and init log config
    // let log_file_path = body.log_file.clone();
    // let log_file_path = PathBuf::from(log_file_path);
    // let log_dir = log_file_path.parent().unwrap().to_path_buf();
    // let log_file_name = log_file_path.file_name().unwrap().to_str().unwrap();

    // log::debug!("update log config");
    // LogConfig::global().lock().update_config(log_file_name, log_dir, None)?;

    log::debug!("run clash core");
    run_core(body).await?;

    Ok(())
}

/// 停止clash进程
pub async fn stop_clash() -> Result<()> {
    log::debug!("stop clash");
    let clash_status = ClashStatus::global();
    if clash_status.sidecar.is_running() {
        clash_status.sidecar.stop().await?;
    }
    Logger::global().clear_log();

    let mut system = System::new();
    system.refresh_all();
    let procs = system.processes_by_name("verge-mihomo".as_ref());
    log::debug!("force kill verge-mihomo process");
    for proc in procs {
        log::debug!("kill {}", proc.name().display());
        proc.kill();
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClashInfo {
    info: Option<StartBody>,
    is_running: bool,
}

/// 获取clash当前执行信息
pub fn get_clash() -> Result<ClashInfo> {
    let clash_status = ClashStatus::global();
    let info = clash_status.info.lock().clone();
    let is_running = clash_status.sidecar.is_running();
    if info.is_none() || !is_running {
        bail!("clash not executed");
    }
    Ok(ClashInfo { info, is_running })
}

/// 获取 logs
pub fn get_logs() -> Result<VecDeque<String>> {
    Ok(Logger::global().get_log())
}
