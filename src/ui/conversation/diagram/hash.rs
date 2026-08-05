use sha2::{Digest, Sha256};

pub fn hash_source(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.trim_end().as_bytes());
    let result = hasher.finalize();
    format!("{result:x}")
}
