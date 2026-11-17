//! The routines of the six pointer slots of `core/lfo` (an interrupt site each) as assembler, one file for each device, in one object (`fills` of `service!`, `[[fill]]` of module.toml
//! name the entries of the device), and the one Rust function they call: `dk_lfo_eval(phase, wave)`, the lookup of the service crate (`eval`). It reads the shape table and writes
//! nothing, so the tick may call it. A host build (the assembly test of the runtime runs the declarations of `service!`) has an empty function for each entry, so that the entry
//! addresses exist.
#[cfg(target_arch = "m68k")]
use core::arch::global_asm;

#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-dt2.s"));
#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-dn2.s"));

/// The first step of `init`: nothing to bind (the slots are named by `fills`; a file without `core/lfo` in the image is refused for its missing slots).
pub unsafe fn setup(_core: &dkrt_svc_rt::ImportCore) -> i32 {
    0
}

macro_rules! entries {
    ($($name:ident,)*) => {
        $(
            #[cfg(target_arch = "m68k")]
            extern "C" {
                pub fn $name();
            }
            #[cfg(not(target_arch = "m68k"))]
            pub extern "C" fn $name() {}
        )*
    };
}

entries! {
    lfo_dt2_tick_call,
    lfo_dt2_tick_slew,
    lfo_dt2_tick_start,
    lfo_dt2_read_call,
    lfo_dt2_read_slew,
    lfo_dt2_read_start,
    lfo_dn2_tick_call,
    lfo_dn2_tick_slew,
    lfo_dn2_tick_start,
    lfo_dn2_read_call,
    lfo_dn2_read_slew,
    lfo_dn2_read_start,
}

#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
/// The value of the shape with wave number `wave` at the 32-bit `phase`, signed Q31 (TRI when no module keeps the number): what the LFO tick takes from the function table for
/// a stock wave; the routines call it for a wave of 7 to 14. C calling convention; the routines of the hooks call it with the phase and the wave on the stack.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub extern "C" fn dk_lfo_eval(phase: u32, wave: u32) -> i32 {
    dkrt_svc_lfo::records::eval(wave, phase)
}
