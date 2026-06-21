use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use std::fs::File;
use std::io::{Read, Write};

const MAGIC: &[u8; 7] = b"SVAULT\x01";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(19 * 1024, 2, 1, Some(32))
        .map_err(|e| anyhow::anyhow!("argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("argon2 hash: {e}"))?;
    Ok(key)
}

pub fn encrypt_bytes(plaintext: &[u8], password: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).context("cipher init")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;

    let mut out = Vec::with_capacity(MAGIC.len() + 1 + SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_bytes(data: &[u8], password: &str) -> Result<Vec<u8>> {
    if data.len() < MAGIC.len() + 1 + SALT_LEN + NONCE_LEN + 16 {
        bail!("archive too small or corrupt");
    }
    if &data[..MAGIC.len()] != MAGIC {
        bail!("not a secret-sweep archive (bad magic)");
    }
    let version = data[MAGIC.len()];
    if version != FORMAT_VERSION {
        bail!("unsupported archive version: {version}");
    }

    let offset = MAGIC.len() + 1;
    let salt = &data[offset..offset + SALT_LEN];
    let nonce_bytes = &data[offset + SALT_LEN..offset + SALT_LEN + NONCE_LEN];
    let ciphertext = &data[offset + SALT_LEN + NONCE_LEN..];

    let key = derive_key(password, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).context("cipher init")?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("decryption failed — wrong password or corrupt archive"))?;
    Ok(plaintext)
}

pub fn write_vault(path: &std::path::Path, inner_zip: &[u8], password: &str) -> Result<()> {
    let encrypted = encrypt_bytes(inner_zip, password)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(&encrypted)?;
    Ok(())
}

pub fn read_vault(path: &std::path::Path, password: &str) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    decrypt_bytes(&data, password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let plain = b"hello secret archive";
        let enc = encrypt_bytes(plain, "test-password-123").unwrap();
        let dec = decrypt_bytes(&enc, "test-password-123").unwrap();
        assert_eq!(dec, plain);
    }
}
