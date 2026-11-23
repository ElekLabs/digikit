//! Store paths: the plain string rules the runtime and the system package share.

/// Is `name` the last part of `path` (the whole of it, or after a `/`)?
#[inline]
pub fn last_part_is(path: &[u8; 64], name: &[u8]) -> bool {
    let mut l = 0;
    while l < 63 && path[l] != 0 {
        l += 1;
    }
    if name.is_empty() || name.len() > l {
        return false;
    }
    let from = l - name.len();
    (from == 0 || path[from - 1] == b'/') && &path[from..l] == name
}

#[cfg(test)]
mod tests;
