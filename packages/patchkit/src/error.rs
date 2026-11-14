use std::fmt;

macro_rules! err {
    ($($t:tt)*) => { $crate::error::Error(format!($($t)*)) };
}

/// Every failure patchkit reports is a message for a person; there is nothing to match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

impl From<dt2_firmware_loader::Error> for Error {
    fn from(e: dt2_firmware_loader::Error) -> Self {
        Error(format!("firmware container: {e}"))
    }
}
impl From<String> for Error {
    fn from(e: String) -> Self {
        Error(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error(format!("json: {e}"))
    }
}
impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Error(format!("toml: {e}"))
    }
}

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err(err!("bad hex string of length {}", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| err!("bad hex digit near {i}")))
        .collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(b))
}
