//! Argon2id password hashing (m = 64 MiB, t = 3, p = 1) and temporary passwords.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::seq::SliceRandom;
use rand::Rng;

use crate::error::{ApiError, ApiResult};

fn hasher() -> Argon2<'static> {
    let params = Params::new(65_536, 3, 1, None).expect("valid argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash(password: &str) -> ApiResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    hasher()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal_msg(format!("password hashing failed: {e}")))
}

pub fn verify(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => hasher()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Hashing is CPU and memory heavy; keep it off the async executor threads.
pub async fn hash_async(password: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || hash(&password))
        .await
        .map_err(|e| ApiError::internal_msg(format!("hash task failed: {e}")))?
}

pub async fn verify_async(password: String, hash: String) -> ApiResult<bool> {
    tokio::task::spawn_blocking(move || verify(&password, &hash))
        .await
        .map_err(|e| ApiError::internal_msg(format!("verify task failed: {e}")))
}

/// Minimum policy: 10 characters with at least one letter and one digit.
pub fn check_strength(password: &str) -> ApiResult<()> {
    let long_enough = password.chars().count() >= 10;
    let has_letter = password.chars().any(|c| c.is_alphabetic());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    if long_enough && has_letter && has_digit {
        Ok(())
    } else {
        Err(ApiError::validation(
            "new_password",
            "must be at least 10 characters and contain a letter and a digit",
        ))
    }
}

/// A 14-character temporary password that satisfies the policy and avoids
/// look-alike characters.
pub fn generate_temporary() -> String {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz";
    const DIGITS: &[u8] = b"23456789";
    const SYMBOLS: &[u8] = b"!@#$%&*";
    let mut rng = rand::thread_rng();
    let mut chars: Vec<u8> = Vec::with_capacity(14);
    for _ in 0..9 {
        chars.push(LETTERS[rng.gen_range(0..LETTERS.len())]);
    }
    for _ in 0..3 {
        chars.push(DIGITS[rng.gen_range(0..DIGITS.len())]);
    }
    for _ in 0..2 {
        chars.push(SYMBOLS[rng.gen_range(0..SYMBOLS.len())]);
    }
    chars.shuffle(&mut rng);
    String::from_utf8(chars).expect("ascii")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let h = hash("Bowline!2026").unwrap();
        assert!(h.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
        assert!(verify("Bowline!2026", &h));
        assert!(!verify("wrong", &h));
    }

    #[test]
    fn temporary_passwords_pass_policy() {
        for _ in 0..20 {
            check_strength(&generate_temporary()).unwrap();
        }
        assert!(check_strength("short1").is_err());
        assert!(check_strength("onlyletterslong").is_err());
    }
}
