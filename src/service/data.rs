use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SocketCommand {
    GetVersion,
    GetClash,
    StartClash(StartBody),
    StopClash,
    StopService,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StartBody {
    pub core_type: Option<String>,
    pub bin_path: String,
    pub config_dir: String,
    pub config_file: String,
    pub log_file: String,
}

// #[derive(Debug, Deserialize, Serialize, Clone)]
// pub struct LogLevelBody {
//     pub level: String,
//     // Is there a need to create a log level for mihomo?
//     // pub mihomo_level: String,
// }

#[derive(Deserialize, Serialize)]
pub struct JsonResponse<T: Serialize> {
    pub code: u64,
    pub msg: String,
    pub data: Option<T>,
}

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
