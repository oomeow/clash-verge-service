use std::str::FromStr;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SocketCommand {
    ClaimClient(ClaimBody),
    Heartbeat(ClientAuthBody),
    ReleaseClient(ClientAuthBody),
    GetVersion,
    GetClash,
    GetLogs,
    StartClash(StartBody),
    StopClash,
    StopService,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClaimBody {
    pub client_id: Vec<u8>,
    pub auth_secret: Vec<u8>,
    pub session_token: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClientAuthBody {
    pub client_id: Vec<u8>,
    pub session_token: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClaimInfo {
    pub client_id: Vec<u8>,
    pub session_token: Vec<u8>,
    pub heartbeat_interval_ms: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceVersionInfo {
    pub version: String,
    pub service: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StartBody {
    pub core_type: Option<String>,
    pub socket_path: Option<String>,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct JsonResponse<T> {
    pub code: u64,
    pub msg: String,
    pub data: Option<T>,
}

impl<T> FromStr for JsonResponse<T>
where
    T: DeserializeOwned,
{
    type Err = serde_json::error::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
