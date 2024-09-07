use super::data::*;
use crate::log_config::{init_log_config, log_expect};
use anyhow::{bail, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use shared_child::SharedChild;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};
use std::thread::spawn;
use sysinfo::System;

#[derive(Debug, Serialize, Clone)]
pub struct ClashStatus {
    pub auto_restart: bool,
    pub restart_retry_count: u32,
    pub info: Option<StartBody>,
}

impl Default for ClashStatus {
    fn default() -> Self {
        ClashStatus {
            auto_restart: false,
            restart_retry_count: 10,
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
    let guard = Arc::new(RwLock::new(()));

    // spawn a thread to read the stdout of the child process
    spawn(move || {
        if let Some(mut output) = child.take_stdout() {
            let _lock = guard.read().unwrap();
            let mut reader = BufReader::new(&mut output).lines();
            while let Some(line) = reader.next() {
                if let Ok(line) = line {
                    wrap_mihomo_log(&line);
                }
            }
        }
    });

    // spawn a thread to wait for the child process to exit
    spawn(move || {
        let _ = child_.wait();
        let mut status = ClashStatus::global().lock();
        if status.auto_restart {
            if status.restart_retry_count > 0 {
                log::debug!(
                    "[clash-verge-service] mihomo terminated, restart count: {}, try to restart...",
                    status.restart_retry_count
                );
                status.restart_retry_count -= 1;
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
    });
    Ok(())
}

/// wrap mihomo log to log::info, log::warn, log::error
fn wrap_mihomo_log(line: &str) {
    let re = Regex::new(r"level=(\w+)").unwrap();
    let level = re
        .captures(line)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str());
    match level {
        Some("info") => {
            log::info!("[mihomo] {}", line);
        }
        Some("warning") => {
            log::warn!("[mihomo] {}", line);
        }
        Some("error") => {
            log::error!("[mihomo] {}", line);
        }
        _ => {
            log::debug!("[mihomo] {}", line);
        }
    }
}

/// POST /start_clash
/// 启动clash进程
pub fn start_clash(body: StartBody) -> Result<()> {
    // stop the old clash bin
    stop_clash()?;
    {
        let mut arc = ClashStatus::global().lock();
        arc.auto_restart = true;
        arc.info = Some(body.clone());
    }
    // get log file path and init log config
    let log_file_path = body.log_file.clone();
    let log_file_path = Path::new(&log_file_path);
    let log_dir = log_file_path.parent().unwrap();
    std::env::set_var("CLASH_VERGE_SERVICE_LOG_DIR", log_dir);
    let log_file_name = log_file_path.file_name().unwrap();
    init_log_config(log_file_name.to_str().unwrap(), None);

    run_core(body)?;

    Ok(())
}

/// POST /stop_clash
/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    {
        let mut arc = ClashStatus::global().lock();
        *arc = ClashStatus::default();
    }

    let mut system = System::new();
    system.refresh_all();
    let procs = system.processes_by_name("verge-mihomo".as_ref());
    for proc in procs {
        proc.kill();
    }
    Ok(())
}

/// GET /get_clash
/// 获取clash当前执行信息
pub fn get_clash() -> Result<ClashStatus> {
    let arc = ClashStatus::global().lock();
    if arc.restart_retry_count <= 0 {
        bail!("clash not executed, retry count exceeded!")
    }
    match (arc.info.clone(), arc.restart_retry_count <= 0) {
        (None, _) => bail!("clash not executed"),
        (Some(_), true) => bail!("clash not executed, retry count exceeded!"),
        (Some(_), false) => Ok(arc.clone()),
    }
}
