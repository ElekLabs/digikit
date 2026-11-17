//! The header of a core library (`sharc-core-lib`): what a host compares with its own values before it
//! uses a `GenCore` from the library.
//!
//! The library carries its own copy of this crate, and only `&mut St` and `fn` pointers with the Rust
//! ABI cross the boundary. That is sound only when both sides were built from the same `sharc-rt`
//! source by the same compiler for the same target, and lay out `St`, `Mem`, `Insn`, `V`, `Cfg` and
//! `GenCore` alike. The header states each of these as a field; [`CoreHeader::first_difference`] names
//! the first field that differs. The function pointer types of `GenCore` and the field types of the
//! generated tables are not fingerprinted one by one: the same source (hashed in `sharc_rt_sha256`),
//! rustc and build flags give them the same types. The strings are NUL-padded ASCII so that the header is `repr(C)` and
//! can be read before any Rust type of the library is trusted.

use crate::BlockFn;
use crate::gencore::GenCore;
use crate::rt::{Cfg, Insn, St, Sym, Trap, V};
use std::mem::{align_of, offset_of, size_of};

/// Version of the header layout itself. A host refuses a header of another version without reading
/// further.
pub const HEADER_ABI: u32 = 4;

pub const SHA_LEN: usize = 64;
pub const RUSTC_LEN: usize = 96;
pub const TARGET_LEN: usize = 64;
pub const FLAGS_LEN: usize = 128;

/// Names of the `layout` entries, in order.
pub const LAYOUT_NAMES: [&str; LAYOUT_LEN] = [
    "St size",
    "St align",
    "St.r offset",
    "St.mem offset",
    "St.cfg offset",
    "St.pc_sw offset",
    "St.icount offset",
    "St.insn_at offset",
    "St.core offset",
    "St.loops offset",
    "St.call_stack offset",
    "St.chain_stop offset",
    "Mem size",
    "Mem align",
    "Mem.pages offset",
    "Mem.eff offset",
    "Mem.ddr_monitor_gen offset",
    "Insn size",
    "Insn align",
    "Insn.type_name offset",
    "Insn.fields offset",
    "Insn.length_bytes offset",
    "Insn.kind offset",
    "Insn.offset offset",
    "V size",
    "V align",
    "V.b offset",
    "V.m offset",
    "Cfg size",
    "Cfg align",
    "Cfg.has_concrete offset",
    "Cfg.max_call_depth offset",
    "Cfg.bank_model offset",
    "GenCore size",
    "GenCore align",
    "GenCore.generated offset",
    "GenCore.blocks offset",
    "GenCore.insn_at offset",
    "GenCore.exec_insn offset",
    "GenCore.packed_counter_laddr offset",
    "Trap size",
    "Trap align",
    "Sym size",
    "Sym align",
    "Option<Insn> size",
    "Option<Insn> align",
    "(u32, BlockFn) size",
    "(u32, BlockFn) align",
    "(u32, u32, &str) size",
    "(u32, u32, &str) align",
    "Option<&[&[u32]]> size",
    "Option<&[&[u32]]> align",
];

pub const LAYOUT_LEN: usize = 52;

/// Size, alignment and selected field offsets of the types that cross the boundary.
pub const LAYOUT: [u64; LAYOUT_LEN] = {
    let mem = crate::mem::LAYOUT;
    [
        size_of::<St>() as u64,
        align_of::<St>() as u64,
        offset_of!(St, r) as u64,
        offset_of!(St, mem) as u64,
        offset_of!(St, cfg) as u64,
        offset_of!(St, pc_sw) as u64,
        offset_of!(St, icount) as u64,
        offset_of!(St, insn_at) as u64,
        offset_of!(St, core) as u64,
        offset_of!(St, loops) as u64,
        offset_of!(St, call_stack) as u64,
        offset_of!(St, chain_stop) as u64,
        mem[0],
        mem[1],
        mem[2],
        mem[3],
        mem[4],
        size_of::<Insn>() as u64,
        align_of::<Insn>() as u64,
        offset_of!(Insn, type_name) as u64,
        offset_of!(Insn, fields) as u64,
        offset_of!(Insn, length_bytes) as u64,
        offset_of!(Insn, kind) as u64,
        offset_of!(Insn, offset) as u64,
        size_of::<V>() as u64,
        align_of::<V>() as u64,
        offset_of!(V, b) as u64,
        offset_of!(V, m) as u64,
        size_of::<Cfg>() as u64,
        align_of::<Cfg>() as u64,
        offset_of!(Cfg, has_concrete) as u64,
        offset_of!(Cfg, max_call_depth) as u64,
        offset_of!(Cfg, bank_model) as u64,
        size_of::<GenCore>() as u64,
        align_of::<GenCore>() as u64,
        offset_of!(GenCore, generated) as u64,
        offset_of!(GenCore, blocks) as u64,
        offset_of!(GenCore, insn_at) as u64,
        offset_of!(GenCore, exec_insn) as u64,
        offset_of!(GenCore, packed_counter_laddr) as u64,
        size_of::<Trap>() as u64,
        align_of::<Trap>() as u64,
        size_of::<Sym>() as u64,
        align_of::<Sym>() as u64,
        size_of::<Option<Insn>>() as u64,
        align_of::<Option<Insn>>() as u64,
        size_of::<(u32, BlockFn)>() as u64,
        align_of::<(u32, BlockFn)>() as u64,
        size_of::<(u32, u32, &str)>() as u64,
        align_of::<(u32, u32, &str)>() as u64,
        size_of::<Option<&[&[u32]]>>() as u64,
        align_of::<Option<&[&[u32]]>>() as u64,
    ]
};

/// The header a core library exports (`sharc_core_header`), and the host's own value.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreHeader {
    pub abi_version: u32,
    /// SHA-256 (hex) of the `sharc-rt` sources (build.rs).
    pub sharc_rt_sha256: [u8; SHA_LEN],
    /// First line of `rustc -vV`'s `rustc ...` line.
    pub rustc: [u8; RUSTC_LEN],
    /// The target triple.
    pub target: [u8; TARGET_LEN],
    /// What changes code generation or layout for the same source, as the build script of `sharc-rt` saw it:
    /// `panic=unwind opt=3 profile=release debug=false rustflags=<16 hex of the SHA-256 of
    /// CARGO_ENCODED_RUSTFLAGS without `-Cprofile-use=`, `-Cprofile-generate=`, `-Cprofile-sample-use=` and the PGO
    /// `-Cllvm-args=` (`-pgo-...`, `-no-pgo-...`); `pgo_flags::strip`>`.
    /// The panic strategy matters because a panic crosses the boundary. PGO changes optimisation, not layout
    /// or ABI, so a PGO host accepts a plain library and the other way round.
    pub flags: [u8; FLAGS_LEN],
    /// `LAYOUT`.
    pub layout: [u64; LAYOUT_LEN],
}

const fn pad<const N: usize>(s: &str) -> [u8; N] {
    let b = s.as_bytes();
    assert!(b.len() <= N, "header string too long");
    let mut out = [0u8; N];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// The header of this copy of the crate.
pub const HOST: CoreHeader = CoreHeader {
    abi_version: HEADER_ABI,
    sharc_rt_sha256: pad(env!("SHARC_RT_SRC_SHA256")),
    rustc: pad(env!("SHARC_RT_RUSTC")),
    target: pad(env!("SHARC_RT_TARGET")),
    flags: pad(env!("SHARC_RT_FLAGS")),
    layout: LAYOUT,
};

impl CoreHeader {
    /// The first field in which `self` (the library's header) differs from `host`, as a message; None
    /// when they are equal.
    pub fn first_difference(&self, host: &CoreHeader) -> Option<String> {
        if self.abi_version != host.abi_version {
            return Some(format!(
                "header version: library {}, host {}",
                self.abi_version, host.abi_version
            ));
        }
        for (name, a, b) in [
            (
                "sharc-rt source sha256",
                &self.sharc_rt_sha256[..],
                &host.sharc_rt_sha256[..],
            ),
            ("rustc", &self.rustc[..], &host.rustc[..]),
            ("target", &self.target[..], &host.target[..]),
            ("build flags", &self.flags[..], &host.flags[..]),
        ] {
            if a != b {
                return Some(format!("{name}: library {}, host {}", text(a), text(b)));
            }
        }
        for (i, name) in LAYOUT_NAMES.iter().enumerate() {
            if self.layout[i] != host.layout[i] {
                return Some(format!(
                    "layout {name}: library {}, host {}",
                    self.layout[i], host.layout[i]
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_headers_do_not_differ() {
        assert_eq!(HOST.first_difference(&HOST), None);
    }

    #[test]
    fn the_first_differing_field_is_named() {
        let mut h = HOST;
        h.layout[3] += 8;
        h.layout[20] += 1;
        let msg = h.first_difference(&HOST).unwrap();
        assert!(msg.starts_with("layout St.mem offset"), "{msg}");
        h.flags = pad("panic=abort");
        let msg = h.first_difference(&HOST).unwrap();
        assert!(msg.starts_with("build flags"), "{msg}");
        h.flags = HOST.flags;
        h.target = pad("other-target");
        assert!(h.first_difference(&HOST).unwrap().starts_with("target"));
        h.sharc_rt_sha256[0] ^= 1;
        assert!(
            h.first_difference(&HOST)
                .unwrap()
                .starts_with("sharc-rt source sha256")
        );
        h.abi_version = 9;
        assert!(
            h.first_difference(&HOST)
                .unwrap()
                .starts_with("header version")
        );
    }
}
