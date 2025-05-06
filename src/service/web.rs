use super::data::*;
use crate::log_config::{log_expect, LogConfig};
use anyhow::{bail, Result};
use regex::Regex;
use shared_child::SharedChild;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::spawn;
use sysinfo::System;

/// GET /version
/// 获取服务进程的版本
pub fn get_version() -> Result<HashMap<String, String>> {
    let version = env!("CARGO_PKG_VERSION");

    let mut map = HashMap::new();
    map.insert("service".into(), "Clash Verge Service".into());
    map.insert("version".into(), version.into());

    Ok(map)
}

fn run_core(body: StartBody) -> Result<()> {
    let body_clone = body.clone();
    let config_dir = body.config_dir.as_str();
    let config_file = body.config_file.as_str();
    let args = vec!["-d", config_dir, "-f", config_file];

    let mut command = Command::new(body.bin_path);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let shared_child = log_expect(SharedChild::spawn(&mut command), "failed to start clash");
    let child = Arc::new(shared_child);
    let child_ = child.clone();

    // spawn a thread to read the stdout of the child process
    spawn(move || {
        if let Some(mut output) = child.take_stdout() {
            let reader = BufReader::new(&mut output).lines();
            for line in reader.map_while(Result::ok) {
                wrap_mihomo_log(&line);
            }
        }
        log::trace!("[clash-verge-service] exited old read core log thread");
    });

    // spawn a thread to wait for the child process to exit
    spawn(move || {
        let _ = child_.wait();
        let status = ClashStatus::global().lock().clone();
        if status.auto_restart {
            if status.restart_retry_count > 0 {
                log::warn!(
                    "[clash-verge-service] mihomo terminated, restart count: {}, try to restart...",
                    status.restart_retry_count
                );
                // update the restart retry count
                {
                    let mut cs = ClashStatus::global().lock();
                    cs.restart_retry_count = status.restart_retry_count - 1;
                }
                if let Err(e) = run_core(body_clone) {
                    log::error!(
                        "[clash-verge-service] failed to restart clash: {}, retry count: {}",
                        e,
                        status.restart_retry_count
                    );
                }
            } else {
                log::error!("[clash-verge-service] failed to restart clash, retry count exceeded!");
                panic!("failed to restart clash, retry count exceeded!");
            }
        }
        log::trace!("[clash-verge-service] exited old restart core thread");
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
        "error" => log::error!(target: "mihomo", "[mihomo] {}", line),
        "warning" => log::warn!(target: "mihomo", "[mihomo] {}", line),
        "info" => log::info!(target: "mihomo", "[mihomo] {}", line),
        "debug" => log::debug!(target: "mihomo", "[mihomo] {}", line),
        _ => log::debug!(target: "mihomo", "[mihomo] {}", line),
    }
}

/// POST /start_clash
/// 启动clash进程
pub fn start_clash(body: StartBody) -> Result<()> {
    // stop the old clash bin
    log::debug!("[clash-verge-service] start clash");
    stop_clash()?;
    {
        let mut arc = ClashStatus::global().lock();
        arc.auto_restart = true;
        arc.info = Some(body.clone());
    }
    // get log file path and init log config
    let log_file_path = body.log_file.clone();
    let log_file_path = PathBuf::from(log_file_path);
    let log_dir = log_file_path.parent().unwrap().to_path_buf();
    let log_file_name = log_file_path.file_name().unwrap().to_str().unwrap();
    log::debug!("[clash-verge-service] update log config");
    LogConfig::global()
        .lock()
        .update_config(log_file_name, log_dir, None)?;

    log::debug!("[clash-verge-service] run clash core");
    run_core(body)?;

    Ok(())
}

/// POST /stop_clash
/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    log::debug!("[clash-verge-service] stop clash");
    // reset the clash status
    {
        let mut arc = ClashStatus::global().lock();
        *arc = ClashStatus::default();
    }

    let mut system = System::new();
    system.refresh_all();
    let procs = system.processes_by_name("verge-mihomo".as_ref());
    log::debug!("[clash-verge-service] kill verge-mihomo process");
    for proc in procs {
        proc.kill();
    }
    Ok(())
}

/// GET /get_clash
/// 获取clash当前执行信息
pub fn get_clash() -> Result<ClashStatus> {
    let arc = ClashStatus::global().lock();
    if arc.restart_retry_count == 0 {
        bail!("clash not executed, retry count exceeded!")
    }
    match (arc.info.clone(), arc.restart_retry_count == 0) {
        (Some(_), false) => Ok(arc.clone()),
        (Some(_), true) => bail!("clash terminated, retry count exceeded!"),
        (None, _) => bail!("clash not executed"),
    }
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
