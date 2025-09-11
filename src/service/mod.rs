pub mod data;
mod handle;
mod logger;

use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};
use bytes::{BufMut, BytesMut};
use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::rand_core::{self, RngCore};
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use data::{JsonResponse, SocketCommand};
use futures::SinkExt;
use futures_util::StreamExt;
pub use handle::ClashStatus;
use handle::{get_clash, get_logs, get_version, start_clash, stop_clash};
use hkdf::Hkdf;
use parking_lot::Mutex;
use tipsy::{Connection, Endpoint, OnConflict, SecurityAttributes, ServerId};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch::{Sender, channel},
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
#[cfg(windows)]
use windows_service::{
    service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType},
    service_control_handler::{self, ServiceControlHandlerResult},
};
use x25519_dalek::{PublicKey, StaticSecret};

#[cfg(windows)]
pub const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
pub const SERVICE_NAME: &str = "clash_verge_service";
pub const DEFAULT_SERVER_ID: &str = "verge-service-server";

const KEY_INFO: &[u8] = b"rust-secure-ipc-demo";

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

pub struct SecureChannel<S> {
    inner: Framed<S, LengthDelimitedCodec>,
    aead: Arc<XChaCha20Poly1305>,
    // 该 IPC 服务不存在大量并发，所以使用 Arc<Mutex<HashSet<u64>>> 已经够用了
    seen_ids: Arc<Mutex<HashSet<u64>>>,
    timestamp_window: u64,
}

impl<S> SecureChannel<S>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    pub async fn send(&mut self, plaintext: &[u8]) -> Result<()> {
        let mut buf = BytesMut::new();

        // timestamp (u64)
        let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        buf.put_u64(ts);

        // message ID (u64 random)
        let mut msg_id = [0u8; 8];
        OsRng.fill_bytes(&mut msg_id);
        buf.put_slice(&msg_id);

        buf.put_slice(plaintext);

        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let cipher = self
            .aead
            .encrypt(&nonce.into(), &buf[..])
            .map_err(|e| anyhow!("encrypt failed: {e}"))?;

        let mut frame = BytesMut::new();
        frame.put_slice(&nonce);
        frame.put_slice(&cipher);
        self.inner.send(frame.freeze()).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        let frame = self.inner.next().await.ok_or(anyhow!("stream closed"))??;
        if frame.len() < 24 {
            return Err(anyhow!("frame too short"));
        }

        let (nonce_bytes, cipher) = frame.split_at(24);
        let plaintext = self
            .aead
            .decrypt(nonce_bytes.into(), cipher)
            .map_err(|e| anyhow!("decrypt failed: {e}"))?;

        if plaintext.len() < 16 {
            return Err(anyhow!("payload too short"));
        }
        let ts = u64::from_be_bytes(plaintext[0..8].try_into()?);
        let msg_id = u64::from_be_bytes(plaintext[8..16].try_into()?);

        // Check timestamp is recent (allow 5s drift) and ID not seen
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if ts + self.timestamp_window < now {
            return Err(anyhow!("replay attack: old timestamp"));
        }

        let mut ids = self.seen_ids.lock();
        if !ids.insert(msg_id) {
            return Err(anyhow!("replay attack: duplicate message ID"));
        }

        Ok(plaintext[16..].to_vec())
    }
}

/// The Service
pub async fn run_service(server_id: Option<String>, psk: Option<&[u8]>) -> Result<()> {
    // 开启服务 设置服务状态
    #[cfg(windows)]
    let status_handle = service_control_handler::register(SERVICE_NAME, move |event| -> ServiceControlHandlerResult {
        match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => std::process::exit(0),
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;
    #[cfg(windows)]
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    let server_id = server_id.unwrap_or(DEFAULT_SERVER_ID.to_string());

    let path = ServerId::new(server_id).parent_folder(std::env::temp_dir());
    let security_attributes = SecurityAttributes::allow_everyone_connect()?;
    let incoming = Endpoint::new(path, OnConflict::Overwrite)?
        .security_attributes(security_attributes)
        .incoming()?;
    futures_util::pin_mut!(incoming);

    let (shutdown_tx, mut shutdown_rx) = channel(());

    tokio::select! {
         _ = async {
            while let Some(result) = incoming.next().await {
                match result {
                    Ok(stream) => {
                        println!("handshake server");
                        let mut secured = SecureChannel::handshake_server(stream, psk).await?;
                        println!("receive request message");
                        if let Ok(msg) = secured.recv().await {
                            println!("server got: {}", String::from_utf8_lossy(&msg));
                            let msg = String::from_utf8_lossy(&msg).to_string();
                            spawn_read_task(msg, secured, shutdown_tx.clone()).await;
                        } else {
                            println!("ca")
                        }
                    }
                    _ => unreachable!("ideally")
                }
            }
            Result::<()>::Ok(())
        } => { }
        _ = shutdown_rx.changed() => {
            let _ = stop_service();
            log::info!("Shutdown Service");
        }
    }

    Ok(())
}

impl SecureChannel<Connection> {
    pub async fn handshake_server(mut stream: Connection, psk: Option<&[u8]>) -> Result<SecureChannel<Connection>> {
        let server_secret = StaticSecret::random_from_rng(rand_core::OsRng);
        let server_pub = PublicKey::from(&server_secret);

        let mut client_pub_bytes = [0u8; 32];
        stream.read_exact(&mut client_pub_bytes).await?;
        let client_pub = PublicKey::from(client_pub_bytes);

        stream.write_all(server_pub.as_bytes()).await?;

        let shared = server_secret.diffie_hellman(&client_pub);
        // derive symmetric key via HKDF-SHA256, mix in PSK as salt if provided
        let hk = match psk {
            Some(salt) => Hkdf::<sha2::Sha256>::new(Some(salt), shared.as_bytes()),
            None => Hkdf::<sha2::Sha256>::new(None, shared.as_bytes()),
        };
        let mut key = [0u8; 32];
        hk.expand(KEY_INFO, &mut key)
            .map_err(|_| anyhow!("hkdf expand failed"))?;

        let aead = XChaCha20Poly1305::new(&key.into());
        let framed = Framed::new(stream, LengthDelimitedCodec::new());
        Ok(SecureChannel {
            inner: framed,
            aead: Arc::new(aead),
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
            timestamp_window: 5,
        })
    }

    pub async fn handshake_client(mut stream: Connection, psk: Option<&[u8]>) -> Result<SecureChannel<Connection>> {
        let client_secret = StaticSecret::random_from_rng(rand_core::OsRng);
        let client_pub = PublicKey::from(&client_secret);

        stream.write_all(client_pub.as_bytes()).await?;

        let mut server_pub_bytes = [0u8; 32];
        stream.read_exact(&mut server_pub_bytes).await?;
        let server_pub = PublicKey::from(server_pub_bytes);

        let shared = client_secret.diffie_hellman(&server_pub);
        // derive symmetric key via HKDF-SHA256, mix in PSK as salt if provided
        let hk = match psk {
            Some(salt) => Hkdf::<sha2::Sha256>::new(Some(salt), shared.as_bytes()),
            None => Hkdf::<sha2::Sha256>::new(None, shared.as_bytes()),
        };
        let mut key = [0u8; 32];
        hk.expand(KEY_INFO, &mut key)
            .map_err(|_| anyhow!("hkdf expand failed"))?;

        let aead = XChaCha20Poly1305::new(&key.into());
        let framed = Framed::new(stream, LengthDelimitedCodec::new());
        Ok(SecureChannel {
            inner: framed,
            aead: Arc::new(aead),
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
            timestamp_window: 5,
        })
    }
}

async fn spawn_read_task(req_data: String, mut secured: SecureChannel<Connection>, shutdown_tx: Sender<()>) {
    tokio::spawn(async move {
        let res: Result<()> = async {
            loop {
                let cmd = serde_json::from_str::<SocketCommand>(&req_data).map_err(|err| {
                    log::error!("Error parsing socket command: {err}");
                    anyhow!("Error parsing socket command: {err}")
                })?;

                handle_socket_command(&mut secured, cmd.clone()).await.map_err(|err| {
                    log::error!("Error handling socket command: {err}");
                    anyhow!("Error handling socket command: {err}")
                })?;
                if let SocketCommand::StopService = cmd {
                    secured.inner.close().await?;
                    let _ = shutdown_tx.send(());
                    break Ok(());
                }
            }
        }
        .await;

        if res.is_err() {
            log::info!("send error response to back");
            let response = wrap_response!(res)?;
            secured.send(response.as_bytes()).await?;
        }

        log::info!("Connection closed");

        Result::<()>::Ok(())
    });
}

/// handle socket command and write response message
async fn handle_socket_command(secured: &mut SecureChannel<Connection>, cmd: SocketCommand) -> Result<()> {
    log::info!("Handling socket command: {cmd:?}");
    let response = match cmd {
        SocketCommand::GetVersion => wrap_response!(get_version())?,
        SocketCommand::GetClash => wrap_response!(get_clash())?,
        SocketCommand::GetLogs => wrap_response!(get_logs())?,
        SocketCommand::StartClash(body) => wrap_response!(start_clash(body))?,
        SocketCommand::StopClash => {
            #[cfg(unix)]
            let socket_path = {
                use crate::service::handle::ClashStatus;

                let clash_status = ClashStatus::global().lock().clone();
                clash_status.info.and_then(|i| i.socket_path)
            };
            let res = wrap_response!(stop_clash())?;
            #[cfg(unix)]
            {
                if let Some(socket_path) = socket_path {
                    log::info!("delete socket path");
                    let path = std::path::Path::new(&socket_path);
                    if path.exists() {
                        std::fs::remove_file(path)?;
                    }
                }
            }
            res
        }
        SocketCommand::StopService => wrap_response!(Result::<()>::Ok(()))?,
    };
    secured.send(response.as_bytes()).await?;
    Ok(())
}

/// 停止服务
#[cfg(windows)]
fn stop_service() -> Result<()> {
    let status_handle = service_control_handler::register(SERVICE_NAME, |_| ServiceControlHandlerResult::NoError)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;
    Ok(())
}

#[cfg(not(windows))]
fn stop_service() -> Result<()> {
    // systemctl stop clash_verge_service
    std::process::Command::new("systemctl")
        .arg("stop")
        .arg(SERVICE_NAME)
        .output()?;
    Ok(())
}
