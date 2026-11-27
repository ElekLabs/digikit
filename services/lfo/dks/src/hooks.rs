//! The routines of the six interrupt hooks of the LFO tick (a site each) as assembler, one file for each device, in one object (`hooks` of `service!`, `[[hook]]` of module.toml
//! name the entries of the device), and the one Rust function they call: `dk_lfo_eval(phase, wave)`, the lookup of the service crate (`eval`). It reads the shape table and writes
//! nothing, so the tick may call it. A host build (the assembly test of the runtime runs the declarations of `service!`) has an empty function for each entry, so that the entry
//! addresses exist.
#[cfg(target_arch = "m68k")]
use core::arch::global_asm;

#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-dt2.s"));
#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-dn2.s"));
#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-ui.s"));

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
    lfo_tick_call_a,
    lfo_tick_slew_a,
    lfo_tick_start_a,
    lfo_read_call_a,
    lfo_read_slew_a,
    lfo_read_start_a,
    lfo_tick_call_b,
    lfo_tick_slew_b,
    lfo_tick_start_b,
    lfo_read_call_b,
    lfo_read_slew_b,
    lfo_read_start_b,
    lfo_ui_name4,
    lfo_ui_name3,
    lfo_ui_graph,
}

/// The value of the shape with wave number `wave` at the 32-bit `phase`, signed Q31 (TRI when no module keeps the number): what the LFO tick takes from the function table for
/// a stock wave; the routines call it for a wave of 7 to 14. C calling convention; the routines of the hooks call it with the phase and the wave on the stack.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub extern "C" fn dk_lfo_eval(phase: u32, wave: u32) -> i32 {
    dkrt_svc_lfo::shapes::eval(wave, phase)
}

/// The name of the shape that the WAVE parameter value `value` (`wave << 8`) names, written to `buf` (`width` characters at most, then a NUL): the number of characters, 0 when no
/// shape is loaded for that wave number. What the name formatters of the MOD page call (`hooks-ui.s`). C calling convention.
///
/// # Safety
/// `buf` is valid for `width + 1` bytes.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_lfo_name(value: i32, buf: *mut u8, width: u32) -> u32 {
    dkrt_svc_lfo::shapes::name_into(value, buf, width as usize) as u32
}

/// The vector of tiles the MOD page draws for the LFO wave `wave` (the unclamped wave argument of its cell), 0 when no loaded shape has that number; stores the unipolar byte at `flag`.
/// `vtable` is the vtable word of the stock tile vector's first `Bitmap`. What the graph hook (`hooks-ui.s`) calls. C calling convention.
///
/// # Safety
/// `flag` is valid for one byte.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_lfo_graph(wave: u32, vtable: u32, flag: *mut u8) -> *const u8 {
    dkrt_svc_lfo::graph::lookup(wave, vtable, flag)
}
