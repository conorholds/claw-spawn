use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use thiserror::Error;
use tracing::warn;

#[derive(Error, Debug)]
pub enum EncryptionError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("Invalid key length")]
    InvalidKeyLength,
}

pub struct SecretsEncryption {
    cipher: Aes256Gcm,
}

impl SecretsEncryption {
    pub fn new(key_base64: &str) -> Result<Self, EncryptionError> {
        let key_bytes = BASE64
            .decode(key_base64)
            .map_err(|_| EncryptionError::InvalidKeyLength)?;

        if key_bytes.len() != 32 {
            return Err(EncryptionError::InvalidKeyLength);
        }

        let key: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| EncryptionError::InvalidKeyLength)?;

        // MED-005: Check key entropy/strength
        Self::validate_key_entropy(&key);

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| EncryptionError::EncryptionFailed(e.to_string()))?;

        Ok(Self { cipher })
    }

    /// Validate key entropy and warn on weak keys (MED-005)
    fn validate_key_entropy(key: &[u8; 32]) {
        // Check for all zeros
        if key.iter().all(|&b| b == 0) {
            warn!("CRITICAL: Encryption key is all zeros - this is extremely insecure!");
            return;
        }

        // Check for all same byte
        let first = key[0];
        if key.iter().all(|&b| b == first) {
            warn!("CRITICAL: Encryption key has uniform values - this is extremely insecure!");
            return;
        }

        // Check for repeating patterns (simple heuristic)
        let mut unique_bytes = std::collections::HashSet::new();
        for &b in key.iter() {
            unique_bytes.insert(b);
        }
        let entropy_ratio = unique_bytes.len() as f32 / key.len() as f32;

        if entropy_ratio < 0.5 {
            warn!(
                "WARNING: Encryption key has low entropy ({:.1}% unique bytes). Consider using a stronger key.",
                entropy_ratio * 100.0
            );
        }

        // Check for common weak patterns
        let printable_only = key
            .iter()
            .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace());
        if printable_only {
            let as_string = String::from_utf8_lossy(key);
            if as_string.contains("password")
                || as_string.contains("secret")
                || as_string.contains("123")
                || as_string.contains("key")
            {
                warn!("WARNING: Encryption key appears to contain dictionary words or common phrases.");
            }
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>, EncryptionError> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| EncryptionError::EncryptionFailed(e.to_string()))?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<String, EncryptionError> {
        if ciphertext.len() < 12 {
            return Err(EncryptionError::DecryptionFailed(
                "Ciphertext too short".to_string(),
            ));
        }

        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| EncryptionError::DecryptionFailed(e.to_string()))?;

        String::from_utf8(plaintext).map_err(|e| EncryptionError::DecryptionFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = "YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXoxMjM0NTY=";
        let encryption = SecretsEncryption::new(key).unwrap();

        let plaintext = "my-secret-api-key-12345";
        let encrypted = encryption.encrypt(plaintext).unwrap();
        let decrypted = encryption.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
    }
}
