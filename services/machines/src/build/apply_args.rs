//! The callback arguments of the ACTION and native page records the commit wrote, for `apply`.
use super::*;

/// The callback arguments of the ACTION and native page records `lower` wrote, in record order (`apply` reads them through `take_args`).
pub struct ApplyArgs {
    pub click: [u32; 32],
    pub native: [u32; 8],
}

pub fn apply_args(b: &Build, plan: &Plan) -> ApplyArgs {
    let mut a = ApplyArgs {
        click: [0; 32],
        native: [0; 8],
    };
    let mut n = 0;
    let mut i = 0;
    while i < b.nparam {
        if b.params[i].click != 0 && plan.file[i] != u8::MAX {
            a.click[n % 32] = b.params[i].click_arg;
            n += 1;
        }
        i += 1;
    }
    n = 0;
    let mut m = 0;
    while m < b.nm {
        let mut i = 0;
        while i < b.np {
            let g = &b.pages[i];
            if g.owner as usize == m && g.canvas {
                a.native[n % 8] = g.arg;
                n += 1;
            }
            i += 1;
        }
        m += 1;
    }
    a
}
