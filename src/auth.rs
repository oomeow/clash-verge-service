use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};

pub const AUTH_KEY_LEN: usize = 32;

const AUTH_KEY_FILE_NAME: &str = "ipc-auth.key";

pub fn load_or_create_auth_key() -> Result<Vec<u8>> {
    let path = auth_key_path();
    match read_auth_key_file(&path) {
        Ok(secret) => Ok(secret),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => match create_auth_key_file(path.clone()) {
            Ok(secret) => Ok(secret),
            Err(err) if is_already_exists(&err) => {
                read_auth_key_file(&path).context("failed to read concurrently created IPC auth key file")
            }
            Err(err) => Err(err).context("failed to create IPC auth key file"),
        },
        Err(err) => Err(err).context("failed to read IPC auth key file"),
    }
}

pub fn load_auth_key() -> Result<Vec<u8>> {
    read_auth_key_file(&auth_key_path()).context("failed to read IPC auth key file")
}

pub fn validate_auth_key(secret: &[u8]) -> Result<()> {
    anyhow::ensure!(
        secret.len() == AUTH_KEY_LEN,
        "invalid IPC auth key length: expected {} bytes, got {} bytes",
        AUTH_KEY_LEN,
        secret.len()
    );
    Ok(())
}

fn auth_key_path() -> PathBuf {
    default_auth_key_dir().join(AUTH_KEY_FILE_NAME)
}

#[cfg(unix)]
fn default_auth_key_dir() -> PathBuf {
    PathBuf::from("/tmp").join("clash-verge-self-service")
}

#[cfg(windows)]
fn default_auth_key_dir() -> PathBuf {
    PathBuf::from(r"C:\ProgramData").join("Clash Verge Self Service")
}

#[cfg(not(any(unix, windows)))]
fn default_auth_key_dir() -> PathBuf {
    PathBuf::from("/tmp").join("clash-verge-self-service")
}

fn read_auth_key_file(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    let mut secret = Vec::new();
    File::open(path)?.read_to_end(&mut secret)?;
    validate_auth_key(&secret).map_err(std::io::Error::other)?;
    Ok(secret)
}

fn create_auth_key_file(path: PathBuf) -> Result<Vec<u8>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
        set_private_dir_permissions(parent)?;
    }

    let secret = generate_auth_key();
    let mut file = create_new_private_file(&path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(&secret)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))?;
    Ok(secret)
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to chmod {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_new_private_file(path: &std::path::Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn create_new_private_file(path: &std::path::Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn is_already_exists(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|err| err.kind() == std::io::ErrorKind::AlreadyExists)
}

fn generate_auth_key() -> Vec<u8> {
    let mut secret = vec![0u8; AUTH_KEY_LEN];
    OsRng.fill_bytes(&mut secret);
    secret
}
