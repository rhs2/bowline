//! Opaque refresh tokens: 32 random bytes, stored as a SHA-256 hex digest.

use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn generate_refresh() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash_refresh(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
