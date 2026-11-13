//! The keys table of a `.dkm` (sdk/sdk.toml `[limits.format]`): the header pair `H_KEYS_OFF`/`H_KEYS_SIZE` locates it. Big-endian:
//! three row counts, the number of machine and effect keys, the size of the record block the module keeps, then the machine keys and the effect keys.
//! A module with no machine and no effect has size 0 and no table.

use crate::MAX_RECORDS;

/// Header fields: offset of the table and its size in bytes.
pub const H_KEYS_OFF: u32 = 0x90;
pub const H_KEYS_SIZE: u32 = 0x94;

/// Field offsets in the table.
pub const K_MACHINE_ROWS: usize = 0;
pub const K_PARAM_ROWS: usize = 2;
pub const K_PAGE_ROWS: usize = 4;
pub const K_N_MACHINES: usize = 6;
pub const K_N_FX: usize = 8;
pub const K_RECORD_BYTES: usize = 10;
pub const K_KEYS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeysError {
    /// The size does not match the counts, or the buffer is too small.
    Size,
    /// A count is above `MAX_RECORDS`.
    Count,
}

/// Size of a table with these counts; 0 when both are 0.
pub fn keys_size(n_machines: usize, n_fx: usize) -> usize {
    if n_machines == 0 && n_fx == 0 {
        0
    } else {
        K_KEYS + 4 * (n_machines + n_fx)
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// A checked view of a keys table.
#[derive(Clone, Copy, Debug)]
pub struct Keys<'a> {
    bytes: &'a [u8],
    n_machines: usize,
    n_fx: usize,
}

impl<'a> Keys<'a> {
    /// `bytes` is exactly the table (`H_KEYS_SIZE` bytes from `H_KEYS_OFF`); size 0 is not a table (the caller skips it).
    pub fn parse(bytes: &'a [u8]) -> Result<Keys<'a>, KeysError> {
        if bytes.len() < K_KEYS {
            return Err(KeysError::Size);
        }
        let n_machines = u16_at(bytes, K_N_MACHINES) as usize;
        let n_fx = u16_at(bytes, K_N_FX) as usize;
        if n_machines > MAX_RECORDS as usize || n_fx > MAX_RECORDS as usize {
            return Err(KeysError::Count);
        }
        if keys_size(n_machines, n_fx) != bytes.len() {
            return Err(KeysError::Size);
        }
        Ok(Keys {
            bytes,
            n_machines,
            n_fx,
        })
    }

    pub fn machine_rows(&self) -> u16 {
        u16_at(self.bytes, K_MACHINE_ROWS)
    }

    pub fn param_rows(&self) -> u16 {
        u16_at(self.bytes, K_PARAM_ROWS)
    }

    pub fn page_rows(&self) -> u16 {
        u16_at(self.bytes, K_PAGE_ROWS)
    }

    /// The payload bytes of the record block the module keeps in the arena after `init` (0: it declares nothing).
    pub fn record_bytes(&self) -> u16 {
        u16_at(self.bytes, K_RECORD_BYTES)
    }

    pub fn n_machines(&self) -> usize {
        self.n_machines
    }

    pub fn n_fx(&self) -> usize {
        self.n_fx
    }

    pub fn machine_keys(&self) -> impl Iterator<Item = u32> + 'a {
        let b = self.bytes;
        (0..self.n_machines).map(move |i| u32_at(b, K_KEYS + 4 * i))
    }

    pub fn fx_keys(&self) -> impl Iterator<Item = u32> + 'a {
        let b = self.bytes;
        let base = K_KEYS + 4 * self.n_machines;
        (0..self.n_fx).map(move |i| u32_at(b, base + 4 * i))
    }
}

/// Writes the table to `out` and returns its size (0, writing nothing, for no machine and no effect). Counts are checked against `MAX_RECORDS`.
pub fn write_keys(
    rows: (u16, u16, u16),
    record_bytes: u16,
    machine_keys: &[u32],
    fx_keys: &[u32],
    out: &mut [u8],
) -> Result<usize, KeysError> {
    let (nm, nf) = (machine_keys.len(), fx_keys.len());
    if nm > MAX_RECORDS as usize || nf > MAX_RECORDS as usize {
        return Err(KeysError::Count);
    }
    let size = keys_size(nm, nf);
    if size > out.len() {
        return Err(KeysError::Size);
    }
    if size == 0 {
        return Ok(0);
    }
    out[..size].fill(0);
    out[K_MACHINE_ROWS..][..2].copy_from_slice(&rows.0.to_be_bytes());
    out[K_PARAM_ROWS..][..2].copy_from_slice(&rows.1.to_be_bytes());
    out[K_PAGE_ROWS..][..2].copy_from_slice(&rows.2.to_be_bytes());
    out[K_RECORD_BYTES..][..2].copy_from_slice(&record_bytes.to_be_bytes());
    out[K_N_MACHINES..][..2].copy_from_slice(&(nm as u16).to_be_bytes());
    out[K_N_FX..][..2].copy_from_slice(&(nf as u16).to_be_bytes());
    for (i, k) in machine_keys.iter().chain(fx_keys).enumerate() {
        out[K_KEYS + 4 * i..][..4].copy_from_slice(&k.to_be_bytes());
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut out = [0xaau8; 64];
        let n = write_keys((2, 3, 4), 700, &[0x11223344, 5], &[0xdeadbeef], &mut out).unwrap();
        assert_eq!(n, 12 + 12);
        assert_eq!(&out[..4], &[0, 2, 0, 3]);
        let k = Keys::parse(&out[..n]).unwrap();
        assert_eq!((k.machine_rows(), k.param_rows(), k.page_rows()), (2, 3, 4));
        assert_eq!(k.record_bytes(), 700);
        assert!(k.machine_keys().eq([0x11223344, 5]));
        assert!(k.fx_keys().eq([0xdeadbeef]));
    }

    #[test]
    fn empty_module_has_no_table() {
        let mut out = [0u8; 16];
        assert_eq!(write_keys((0, 0, 0), 0, &[], &[], &mut out), Ok(0));
        assert_eq!(Keys::parse(&[]).unwrap_err(), KeysError::Size);
    }

    #[test]
    fn bad_tables_are_refused() {
        let mut out = [0u8; 32];
        let n = write_keys((0, 0, 0), 0, &[1], &[2], &mut out).unwrap();
        assert_eq!(Keys::parse(&out[..n - 4]).unwrap_err(), KeysError::Size);
        assert_eq!(Keys::parse(&out[..n + 4]).unwrap_err(), KeysError::Size);
        let mut many = out;
        many[K_N_FX + 1] = 33;
        assert_eq!(Keys::parse(&many[..n]).unwrap_err(), KeysError::Count);
        assert_eq!(
            write_keys((0, 0, 0), 0, &[1], &[2], &mut out[..n - 1]),
            Err(KeysError::Size)
        );
    }
}
