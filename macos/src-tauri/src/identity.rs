use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{crypto, db};

pub const NOISE_PROTOCOL: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const KEY_SIZE: usize = 32;

#[derive(Clone)]
pub struct DeviceIdentity {
    private_key: Vec<u8>,
    public_key: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    version: u8,
    private_key: String,
    public_key: String,
}

impl DeviceIdentity {
    #[cfg(test)]
    pub(crate) fn generate_for_test() -> Self {
        let params = NOISE_PROTOCOL.parse().expect("valid Noise parameters");
        let keypair = snow::Builder::new(params)
            .generate_keypair()
            .expect("generate test identity");
        Self {
            private_key: keypair.private,
            public_key: keypair.public,
        }
    }

    pub fn load_or_create() -> Result<Self, Box<dyn std::error::Error>> {
        let path = identity_path();
        if path.exists() {
            return Self::load(&path);
        }

        let params = NOISE_PROTOCOL.parse()?;
        let keypair = snow::Builder::new(params).generate_keypair()?;
        let identity = Self {
            private_key: keypair.private,
            public_key: keypair.public,
        };
        identity.validate()?;
        identity.save(&path)?;
        Ok(identity)
    }

    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(&self.public_key)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public_key)
    }

    fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let encrypted = std::fs::read(path)?;
        let plaintext = crypto::decrypt(&encrypted)?;
        let stored: StoredIdentity = serde_json::from_slice(&plaintext)?;
        if stored.version != 1 {
            return Err(format!("Unsupported device identity version: {}", stored.version).into());
        }
        let identity = Self {
            private_key: STANDARD.decode(stored.private_key)?,
            public_key: STANDARD.decode(stored.public_key)?,
        };
        identity.validate()?;
        Ok(identity)
    }

    fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let stored = StoredIdentity {
            version: 1,
            private_key: STANDARD.encode(&self.private_key),
            public_key: STANDARD.encode(&self.public_key),
        };
        let encrypted = crypto::encrypt(&serde_json::to_vec(&stored)?)?;
        let temporary = path.with_extension("bin.tmp");
        std::fs::write(&temporary, encrypted)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.private_key.len() != KEY_SIZE || self.public_key.len() != KEY_SIZE {
            return Err("Invalid X25519 device identity".into());
        }
        Ok(())
    }
}

pub fn decode_public_key(encoded: &str) -> Result<Vec<u8>, String> {
    let key = STANDARD
        .decode(encoded.trim())
        .map_err(|_| "Device public key is not valid Base64".to_string())?;
    if key.len() != KEY_SIZE {
        return Err("Device public key must decode to 32 bytes".to_string());
    }
    Ok(key)
}

pub fn canonical_public_key(encoded: &str) -> Result<String, String> {
    decode_public_key(encoded).map(|key| STANDARD.encode(key))
}

pub fn fingerprint(public_key: &[u8]) -> String {
    let hash = blake3::hash(public_key);
    let short = hex::encode(&hash.as_bytes()[..10]).to_uppercase();
    short
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).expect("hex is ASCII"))
        .collect::<Vec<_>>()
        .join("-")
}

fn identity_path() -> PathBuf {
    db::get_data_dir().join("identity-v1.bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_validation_is_canonical_and_fingerprinted() {
        let key = [0x5a; KEY_SIZE];
        let encoded = STANDARD.encode(key);
        assert_eq!(canonical_public_key(&encoded).unwrap(), encoded);
        assert_eq!(decode_public_key(&encoded).unwrap(), key);
        assert_eq!(fingerprint(&key).split('-').count(), 5);
        assert!(decode_public_key("not-a-key").is_err());
    }
}
