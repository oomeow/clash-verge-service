use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use anyhow::{Ok, Result, anyhow, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use rsa::Pkcs1v15Encrypt;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, EncodeRsaPublicKey};
use rsa::{RsaPrivateKey, RsaPublicKey, pkcs1::DecodeRsaPublicKey};
use std::env::current_exe;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

pub const PRI_KEY_PEM_FILE: &str = ".private.pem";
pub const PUB_KEY_PEM_FILE: &str = ".public.pem";

// pub fn init_rsa_keys() -> Result<()> {
//     let private_key_path = Path::new(&PRI_KEY_PEM_PATH);
//     let public_key_path = Path::new(&PUB_KEY_PEM_PATH);
//     if !private_key_path.exists() || !public_key_path.exists() {
//         generate_rsa_keys()?;
//     }
//     Ok(())
// }

pub fn generate_rsa_keys() -> Result<(RsaPrivateKey, RsaPublicKey)> {
    let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)?;
    let public_key = RsaPublicKey::from(&private_key);
    let current_exe_path = current_exe()?;
    let current_exe_dir = current_exe_path.parent();
    let (pri_key_path, pub_key_path) = match current_exe_dir {
        Some(current_exe_dir) => (
            current_exe_dir.join(PRI_KEY_PEM_FILE),
            current_exe_dir.join(PUB_KEY_PEM_FILE),
        ),
        None => (
            PathBuf::from_str(PRI_KEY_PEM_FILE)?,
            PathBuf::from_str(PUB_KEY_PEM_FILE)?,
        ),
    };
    std::fs::write(pri_key_path, &private_key.to_pkcs1_pem(Default::default())?)?;
    std::fs::write(pub_key_path, &public_key.to_pkcs1_pem(Default::default())?)?;
    Ok((private_key, public_key))
}

pub fn load_keys() -> Result<(RsaPrivateKey, RsaPublicKey)> {
    let current_exe_path = current_exe()?;
    let current_exe_dir = current_exe_path.parent();
    let (pri_key_path, pub_key_path) = match current_exe_dir {
        Some(current_exe_dir) => (
            current_exe_dir.join(PRI_KEY_PEM_FILE),
            current_exe_dir.join(PUB_KEY_PEM_FILE),
        ),
        None => (
            PathBuf::from_str(PRI_KEY_PEM_FILE)?,
            PathBuf::from_str(PUB_KEY_PEM_FILE)?,
        ),
    };

    let mut pri_key_file = std::fs::File::open(pri_key_path)?;
    let mut private_key_content = String::new();
    pri_key_file.read_to_string(&mut private_key_content)?;
    let private_key = RsaPrivateKey::from_pkcs1_pem(&private_key_content)?;

    let mut pub_key_file = std::fs::File::open(pub_key_path)?;
    let mut public_key_content = String::new();
    pub_key_file.read_to_string(&mut public_key_content)?;
    let public_key = RsaPublicKey::from_pkcs1_pem(&public_key_content)?;

    Ok((private_key, public_key))
}

pub fn rsa_encrypt(public_key: &RsaPublicKey, data: &[u8]) -> Result<Vec<u8>> {
    Ok(public_key
        .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, data)
        .map_err(|e| anyhow::anyhow!("rsa encrypt failed, error {:?}", e))?)
}

pub fn rsa_decrypt(private_key: &RsaPrivateKey, enc_data: &[u8]) -> Result<Vec<u8>> {
    Ok(private_key
        .decrypt(Pkcs1v15Encrypt, enc_data)
        .map_err(|e| anyhow::anyhow!("rsa decrypt failed, error {:?}", e))?)
}

pub fn aes_encrypt(key: &[u8], nonce: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());
    Ok(cipher
        .encrypt(Nonce::from_slice(nonce), data)
        .map_err(|e| anyhow::anyhow!("aes encrypt failed, error {:?}", e))?)
}

pub fn aes_decrypt(key: &[u8], nonce: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());
    Ok(cipher
        .decrypt(Nonce::from_slice(nonce), data)
        .map_err(|e| anyhow::anyhow!("aes decrypt failed, error {:?}", e))?)
}

pub fn gen_aes_key_and_nonce() -> (Vec<u8>, Vec<u8>) {
    let key = Aes256Gcm::generate_key(&mut OsRng);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    (key.to_vec(), nonce.to_vec())
}

pub fn encrypt_socket_data(public_key: &RsaPublicKey, data: &str) -> Result<String> {
    let (aes_key, nonce) = gen_aes_key_and_nonce();
    let ciphertext = aes_encrypt(&aes_key, &nonce, data.as_bytes())?;
    let enc_key = rsa_encrypt(public_key, &aes_key)?;

    let combined = format!(
        "{}|{}|{}\n",
        BASE64_STANDARD.encode(&enc_key),
        BASE64_STANDARD.encode(&nonce),
        BASE64_STANDARD.encode(&ciphertext)
    );

    Ok(combined)
}

pub fn decrypt_socket_data(private_key: &RsaPrivateKey, data: &str) -> Result<String> {
    let parts: Vec<&str> = data.trim().split('|').collect();
    if parts.len() != 3 {
        bail!("Invalid format");
    }

    let enc_data = BASE64_STANDARD.decode(parts[0])?;
    let nonce = BASE64_STANDARD.decode(parts[1])?;
    let ciphertext = BASE64_STANDARD.decode(parts[2])?;

    let aes_key = rsa_decrypt(private_key, &enc_data)
        .map_err(|e| anyhow!("decrypt data failed, error: {}", e))?;
    let plaintext = aes_decrypt(&aes_key, &nonce, &ciphertext)
        .map_err(|e| anyhow!("decrypt data failed, error: {}", e))?;
    let data = String::from_utf8_lossy(&plaintext);

    Ok(data.to_string())
}
