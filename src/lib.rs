mod auth;
mod ipc;
pub mod log_config;
mod process_identity;
pub mod runtime;
mod service;

use std::str::FromStr;
use std::time::Duration;

use anyhow::{Result, anyhow};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use serde::de::DeserializeOwned;
use tokio::{
    sync::{mpsc, oneshot},
    time::MissedTickBehavior,
};

pub mod model {
    pub use super::service::{ClashRunInfo, data::*};
}

#[cfg(windows)]
use windows_service::service::ServiceType;

use crate::service::SecureChannel;

#[cfg(windows)]
pub const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
pub const SERVICE_NAME: &str = "clash_verge_self_service";

// default server id
pub const DEFAULT_SERVER_ID: &str = "verge-self-service-server";

// encode relate
const KEY_INFO: &[u8] = b"rust-secure-ipc-demo";

pub struct Client {
    request_tx: mpsc::Sender<ClientRequest>,
    heartbeat_interval: Duration,
    lease_ttl: Duration,
}

struct ClientRequest {
    command: model::SocketCommand,
    response_tx: oneshot::Sender<Result<String>>,
}

impl Client {
    /// connect to server
    ///
    /// unix system: `/tmp/{server_id}.sock`
    ///
    /// Windows system: `\\.\pipe\{server_id}`
    pub async fn connect<S: Into<String>>(server_id: S) -> Result<Self> {
        let stream = ipc::connect(server_id.into()).await?;
        let mut secured = SecureChannel::handshake_client(stream).await?;
        let auth_secret = auth::load_or_create_auth_key()?;
        let client_id = generate_client_id();
        let claim = claim_service(&mut secured, client_id.clone(), auth_secret).await?;

        let heartbeat_interval = Duration::from_millis(claim.heartbeat_interval_ms);
        let lease_ttl = Duration::from_millis(claim.lease_ttl_ms);
        let (request_tx, request_rx) = mpsc::channel(32);
        runtime::spawn(run_client_task(
            secured,
            claim.client_id,
            claim.session_token,
            heartbeat_interval,
            request_rx,
        ));

        Ok(Self {
            request_tx,
            heartbeat_interval,
            lease_ttl,
        })
    }

    pub fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_interval
    }

    pub fn lease_ttl(&self) -> Duration {
        self.lease_ttl
    }

    pub async fn heartbeat(&self) -> Result<model::JsonResponse<()>> {
        self.send_command(model::SocketCommand::Heartbeat(model::ClientAuthBody {
            client_id: Vec::new(),
            session_token: Vec::new(),
        }))
        .await
    }

    pub async fn release(&self) -> Result<model::JsonResponse<()>> {
        self.send_command(model::SocketCommand::ReleaseClient(model::ClientAuthBody {
            client_id: Vec::new(),
            session_token: Vec::new(),
        }))
        .await
    }

    /// send socket command request
    pub async fn send<T: DeserializeOwned>(&self, command: model::SocketCommand) -> Result<model::JsonResponse<T>> {
        self.send_command(command).await
    }

    async fn send_command<T: DeserializeOwned>(&self, command: model::SocketCommand) -> Result<model::JsonResponse<T>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.request_tx
            .send(ClientRequest { command, response_tx })
            .await
            .map_err(|_| anyhow!("client task closed"))?;
        let msg = response_rx.await.map_err(|_| anyhow!("client task closed"))??;
        let res = model::JsonResponse::from_str(&msg)?;
        Ok(res)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let request_tx = self.request_tx.clone();
        runtime::block_on(async move {
            let (response_tx, response_rx) = oneshot::channel();
            let request = ClientRequest {
                command: model::SocketCommand::ReleaseClient(model::ClientAuthBody {
                    client_id: Vec::new(),
                    session_token: Vec::new(),
                }),
                response_tx,
            };
            if request_tx.send(request).await.is_ok()
                && let Err(err) = response_rx.await
            {
                log::warn!("failed to receive release response: {err}");
            }
        });
    }
}

async fn claim_service(
    secured: &mut SecureChannel,
    client_id: Vec<u8>,
    auth_secret: Vec<u8>,
) -> Result<model::ClaimInfo> {
    let claim = send_secure_command::<model::ClaimInfo>(
        secured,
        model::SocketCommand::ClaimClient(model::ClaimBody {
            client_id,
            auth_secret,
            session_token: None,
        }),
    )
    .await?;

    claim
        .data
        .ok_or_else(|| anyhow!("failed to claim service: {}", claim.msg))
}

async fn run_client_task(
    mut secured: SecureChannel,
    client_id: Vec<u8>,
    session_token: Vec<u8>,
    heartbeat_interval: Duration,
    mut request_rx: mpsc::Receiver<ClientRequest>,
) {
    let mut heartbeat = tokio::time::interval(heartbeat_interval);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            request = request_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                let should_release = matches!(request.command, model::SocketCommand::ReleaseClient(_));
                let result = send_secure_command_raw(
                    &mut secured,
                    attach_client_auth(request.command, &client_id, &session_token),
                )
                .await;
                let _ = request.response_tx.send(result);
                if should_release {
                    break;
                }
            }
            _ = heartbeat.tick() => {
                let command = model::SocketCommand::Heartbeat(model::ClientAuthBody {
                    client_id: client_id.clone(),
                    session_token: session_token.clone(),
                });
                match send_secure_command::<()>(&mut secured, command).await {
                    Ok(response) if response.code == 0 => {}
                    Ok(response) => {
                        log::error!("client heartbeat failed: {}", response.msg);
                        break;
                    }
                    Err(err) => {
                        log::error!("client heartbeat failed: {err}");
                        break;
                    }
                }
            }
        }
    }
}

fn attach_client_auth(command: model::SocketCommand, client_id: &[u8], session_token: &[u8]) -> model::SocketCommand {
    match command {
        model::SocketCommand::Heartbeat(_) => model::SocketCommand::Heartbeat(model::ClientAuthBody {
            client_id: client_id.to_vec(),
            session_token: session_token.to_vec(),
        }),
        model::SocketCommand::ReleaseClient(_) => model::SocketCommand::ReleaseClient(model::ClientAuthBody {
            client_id: client_id.to_vec(),
            session_token: session_token.to_vec(),
        }),
        command => command,
    }
}

async fn send_secure_command<T: DeserializeOwned>(
    secured: &mut SecureChannel,
    command: model::SocketCommand,
) -> Result<model::JsonResponse<T>> {
    let msg = send_secure_command_raw(secured, command).await?;
    Ok(model::JsonResponse::from_str(&msg)?)
}

async fn send_secure_command_raw(secured: &mut SecureChannel, command: model::SocketCommand) -> Result<String> {
    let cmd_json = serde_json::to_string(&command)?;
    secured.send(cmd_json.as_bytes()).await?;
    let res = secured.recv().await?;
    let msg = String::from_utf8(res)?;
    log::info!("connect to service success");
    Ok(msg)
}

fn generate_client_id() -> Vec<u8> {
    let mut client_id = vec![0u8; 16];
    OsRng.fill_bytes(&mut client_id);
    client_id
}

pub struct Server;

impl Server {
    /// run server
    pub async fn run<S: Into<String>>(server_id: S) -> Result<()> {
        service::run_service(Some(server_id.into())).await?;
        Ok(())
    }
}
