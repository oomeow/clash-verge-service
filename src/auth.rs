use anyhow::{Context, Result};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use keyring::{Entry, Error};

pub const AUTH_KEY_LEN: usize = 32;

const KEYRING_SERVICE: &str = "clash-verge-self-service";
const KEYRING_ACCOUNT: &str = "ipc-auth-key";

pub fn load_or_create_auth_key() -> Result<Vec<u8>> {
    let entry = auth_key_entry()?;
    match entry.get_secret() {
        Ok(secret) => {
            println!("sercret: {secret:?}");
            validate_auth_key(&secret)?;
            Ok(secret)
        }
        Err(Error::NoEntry) => {
            let secret = generate_auth_key();
            entry
                .set_secret(&secret)
                .context("failed to store IPC auth key in keyring")?;
            Ok(secret)
        }
        Err(err) => Err(err).context("failed to read IPC auth key from keyring"),
    }
}

fn auth_key_entry() -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).context("failed to open IPC auth key keyring entry")
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

fn generate_auth_key() -> Vec<u8> {
    let mut secret = vec![0u8; AUTH_KEY_LEN];
    OsRng.fill_bytes(&mut secret);
    secret
}
