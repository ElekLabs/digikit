use crate::*;

/// Text for a result code, for a menu line (upper case, at most 14 characters). A table, not a `match`: the compiler turns a
/// dense `match` into a PC-relative jump table that is not ColdFire code.
pub fn result_text(code: i32) -> &'static str {
    const TEXTS: [&str; 15] = [
        "OK",
        "NOT FOUND",
        "BAD STORE",
        "READ ERROR",
        "BAD SHA",
        "BAD FORMAT",
        "RUNTIME OLD",
        "NO IMPORT",
        "NO MEMORY",
        "INIT FAILED",
        "ALREADY LOADED",
        "ERROR",
        "BUSY",
        "TOO BIG",
        "TABLE FULL",
    ];
    const DSP_TEXTS: [&str; 8] = [
        "NO DSP LINK",
        "BAD DSP CODE",
        "DSP IMPORT",
        "DSP FULL",
        "NO VOICE ROW",
        "DSP TRANSFER",
        "WRONG DEVICE",
        "BAD VOICE",
    ];
    if (E_DSP_TYPE..=E_NO_DSP).contains(&code) {
        DSP_TEXTS[(-code - 30) as usize]
    } else if code == LOADING {
        "LOADING"
    } else if (E_TABLE_FULL..=OK).contains(&code) {
        TEXTS[(-code) as usize]
    } else {
        "ERROR"
    }
}
