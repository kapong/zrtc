use sha2::{Sha256, Digest};

const ALPHANUMERIC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Generate a random alphanumeric string of given length.
pub fn generate_random_string(length: usize) -> String {
    let mut buf = vec![0u8; length];
    getrandom::getrandom(&mut buf).expect("getrandom failed");
    buf.iter()
        .map(|b| ALPHANUMERIC[(*b as usize) % ALPHANUMERIC.len()] as char)
        .collect()
}

/// Generate 16 random bytes for passcode salt.
pub fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).expect("getrandom failed");
    salt
}

/// SHA-256(passcode_bytes + salt_bytes) → 32-byte hash as hex string.
pub fn hash_passcode(passcode: &str, salt: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(passcode.as_bytes());
    hasher.update(salt);
    hex::encode(hasher.finalize())
}

/// Verify passcode against stored hash + salt.
pub fn verify_passcode(passcode: &str, salt: &[u8], expected_hash: &str) -> bool {
    let computed = hash_passcode(passcode, salt);
    // Constant-time comparison via iterating all bytes
    if computed.len() != expected_hash.len() {
        return false;
    }
    computed
        .bytes()
        .zip(expected_hash.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
