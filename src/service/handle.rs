use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    thread::spawn,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use shared_child::SharedChild;
use sysinfo::System;

use super::data::*;
use crate::{log_config::LogConfig, service::logger::Logger};

/// 默认重新运行的尝试次数
const DEFAULT_RETRY_COUNT: u8 = 10;

/// 重置 restart_retry_count 的间隔时间，通过当前重试的时间与上一次运行时间的时间间隔做比对
const INTERVAL_TIME: f64 = 60.0;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClashStatus {
    pub auto_restart: bool,
    pub restart_retry_count: u8,
    #[serde(skip)]
    pub child: Arc<Mutex<Option<Arc<SharedChild>>>>,
    pub last_running_time: DateTime<Local>,
    pub info: Option<StartBody>,
}

impl Default for ClashStatus {
    fn default() -> Self {
        ClashStatus {
            auto_restart: false,
            restart_retry_count: DEFAULT_RETRY_COUNT,
            child: Arc::new(Mutex::new(None)),
            last_running_time: Local::now(),
            info: None,
        }
    }
}

impl ClashStatus {
    pub fn global() -> &'static Arc<Mutex<ClashStatus>> {
        static CLASHSTATUS: OnceCell<Arc<Mutex<ClashStatus>>> = OnceCell::new();
        CLASHSTATUS.get_or_init(|| Arc::new(Mutex::new(ClashStatus::default())))
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

fn run_core(body: StartBody) -> Result<()> {
    let body_clone = body.clone();
    let config_dir = body.config_dir.as_str();
    let config_file = body.config_file.as_str();
    let mut args = vec!["-d", config_dir, "-f", config_file];
    if let Some(socket_path) = body.socket_path.as_ref() {
        #[cfg(unix)]
        args.push("-ext-ctl-unix");
        #[cfg(windows)]
        args.push("-ext-ctl-pipe");
        args.push(socket_path);
    }

    let mut command = Command::new(body.bin_path);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let shared_child = SharedChild::spawn(&mut command).context("failed to spawn clash")?;
    let child = Arc::new(shared_child);
    {
        let mut clash_status = ClashStatus::global().lock();
        clash_status.child = Arc::new(Mutex::new(Some(child.clone())));
        clash_status.last_running_time = Local::now();
    }
    let child_ = child.clone();

    // spawn a thread to read the stdout of the child process
    spawn(move || {
        if let Some(mut output) = child.take_stdout() {
            let reader = BufReader::new(&mut output).lines();
            for line in reader.map_while(Result::ok) {
                Logger::global().set_log(line.clone());
                wrap_mihomo_log(&line);
            }
        }
        log::trace!("exited old read core log thread");
    });

    // spawn a thread to wait for the child process to exit
    spawn(move || {
        let _ = child_.wait();
        let mut clash_status = ClashStatus::global().lock().clone();
        if clash_status.auto_restart {
            let now = Local::now();
            let elapsed = (now - clash_status.last_running_time).as_seconds_f64();
            log::info!("elapsed time from last running time: {elapsed} seconds");
            if elapsed > INTERVAL_TIME {
                log::info!(
                    "elapsed time greater than {INTERVAL_TIME} seconds, reset retry count to {DEFAULT_RETRY_COUNT}",
                );
                // update the restart retry count
                let mut clash_status_ = ClashStatus::global().lock();
                clash_status_.restart_retry_count = DEFAULT_RETRY_COUNT;
                clash_status = clash_status_.clone();
            }
            if clash_status.restart_retry_count > 0 {
                log::warn!(
                    "mihomo terminated, attempt to restart {}/{}...",
                    clash_status.restart_retry_count,
                    DEFAULT_RETRY_COUNT
                );
                {
                    // update the restart retry count
                    let mut clash_status = ClashStatus::global().lock();
                    clash_status.restart_retry_count -= 1;
                }
                Logger::global().clear_log();
                if let Err(e) = run_core(body_clone) {
                    log::error!("failed to restart clash: {e}");
                }
            } else {
                log::error!("failed to restart clash, retry count exceeded!");
            }
        }
        log::trace!("exited old restart core thread");
    });
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
pub fn start_clash(body: StartBody) -> Result<()> {
    // stop the old clash bin
    log::debug!("start clash {body:?}");
    stop_clash()?;
    {
        let mut clash_status = ClashStatus::global().lock();
        clash_status.auto_restart = true;
        clash_status.info = Some(body.clone());
    }
    // get log file path and init log config
    let log_file_path = body.log_file.clone();
    let log_file_path = PathBuf::from(log_file_path);
    let log_dir = log_file_path.parent().unwrap().to_path_buf();
    let log_file_name = log_file_path.file_name().unwrap().to_str().unwrap();
    log::debug!("update log config");
    LogConfig::global().lock().update_config(log_file_name, log_dir, None)?;

    log::debug!("run clash core");
    run_core(body)?;

    Ok(())
}

/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    log::debug!("stop clash");
    {
        // reset the clash status
        let mut arc = ClashStatus::global().lock();
        if let Some(child) = arc.child.lock().take() {
            log::info!("stop clash by use shared child");
            child.kill()?;
        }
        *arc = ClashStatus::default();
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

/// 获取clash当前执行信息
pub fn get_clash() -> Result<ClashStatus> {
    let clash_status = ClashStatus::global().lock();
    if clash_status.restart_retry_count == 0 {
        bail!("clash not executed, retry count exceeded!")
    }
    match (clash_status.info.clone(), clash_status.restart_retry_count == 0) {
        (Some(_), false) => Ok(clash_status.clone()),
        (Some(_), true) => bail!("clash terminated, retry count exceeded!"),
        (None, _) => bail!("clash not executed"),
    }
}

/// 获取 logs
pub fn get_logs() -> Result<VecDeque<String>> {
    Ok(Logger::global().get_log())
}

// pub fn update_log_level(body: LogLevelBody) -> Result<()> {
//     let log_level = body.level;
//     let log_level = match log_level.as_str() {
//         "off" => log::LevelFilter::Off,
//         "error" => log::LevelFilter::Error,
//         "warn" => log::LevelFilter::Warn,
//         "info" => log::LevelFilter::Info,
//         "debug" => log::LevelFilter::Debug,
//         "trace" => log::LevelFilter::Trace,
//         _ => bail!("invalid log level"),
//     };
//     LogConfig::global().lock().update_log_level(log_level)?;
//     Ok(())
// }
