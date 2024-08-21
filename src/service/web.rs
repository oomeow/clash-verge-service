use super::data::*;
use anyhow::{bail, Context, Ok, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::process::Command;
use std::sync::Arc;
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
    let log = File::create(body.log_file).context("failed to open log")?;

    let mut child = Command::new(body.bin_path).args(args).stdout(log).spawn()?;
    tokio::spawn(async move {
        let _ = child.wait();
        let mut status = ClashStatus::global().lock();
        if status.auto_restart {
            if status.restart_retry_count > 0 {
                status.restart_retry_count -= 1;
                run_core(body_clone).expect("failed to restart clash");
            } else {
                panic!("failed to restart clash, retry count exceeded!");
            }
        }
    });
    Ok(())
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
