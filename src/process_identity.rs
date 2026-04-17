use anyhow::{Context, Result, anyhow};

use crate::ipc::Connection;

#[cfg(unix)]
pub fn verify_connection_identity(stream: &Connection) -> Result<()> {
    let peer = stream
        .unix_stream()
        .peer_cred()
        .context("failed to read IPC peer credentials")?;
    let current_uid = unsafe { libc::geteuid() };

    if peer.uid() != current_uid {
        return Err(anyhow!(
            "IPC peer uid mismatch: expected {}, got {}",
            current_uid,
            peer.uid()
        ));
    }

    #[cfg(target_os = "macos")]
    if peer.pid().is_none() {
        return Err(anyhow!("IPC peer pid is unavailable"));
    }

    Ok(())
}

#[cfg(windows)]
pub fn verify_connection_identity(stream: &Connection) -> Result<()> {
    stream.verify_windows_client_token()
}

#[cfg(not(any(unix, windows)))]
pub fn verify_connection_identity(_stream: &Connection) -> Result<()> {
    Err(anyhow!(
        "IPC peer identity verification is not implemented on this platform"
    ))
}
