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
#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks-ui.s"));

#[cfg(target_arch = "m68k")]
extern "C" {
    /// Four words of the asm of `hooks-ui.s`: the stock parameter table and the rows of the three WAVE parameters (`dk_fw_` words, resolved by the port file).
    static DKRT_LFO_FW: [u32; 4];
}

/// The first step of `init`: give the service the firmware addresses of the WAVE rows (the slots are named by `fills`; a file without `core/lfo` in the image is refused for its
/// missing slots). A host build has no firmware: nothing is set and no row is written.
pub unsafe fn setup(_core: &dkrt_svc_rt::ImportCore) -> i32 {
    #[cfg(target_arch = "m68k")]
    dkrt_svc_lfo::records::set_firmware(
        core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[0])),
        [
            core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[1])),
            core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[2])),
            core::ptr::read_volatile(core::ptr::addr_of!(DKRT_LFO_FW[3])),
        ],
    );
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
    lfo_ui_name4,
    lfo_ui_name3,
    lfo_ui_graph,
}

#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
/// The value of the shape with wave number `wave` at the 32-bit `phase`, signed Q31 (TRI when no module keeps the number): what the LFO tick takes from the function table for
/// a stock wave; the routines call it for a wave of 7 to 14. C calling convention; the routines of the hooks call it with the phase and the wave on the stack.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub extern "C" fn dk_lfo_eval(phase: u32, wave: u32) -> i32 {
    dkrt_svc_lfo::records::eval(wave, phase)
}

/// The name of the shape that the WAVE parameter value `value` (`wave << 8`) names, written to `buf` (`width` characters at most, then a NUL): the number of characters, 0 when no
/// shape is loaded for that wave number. What the name formatters of the MOD page call (`hooks-ui.s`). C calling convention.
///
/// # Safety
/// `buf` is valid for `width + 1` bytes.
#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_lfo_name(value: i32, buf: *mut u8, width: u32) -> u32 {
    dkrt_svc_lfo::records::name_into(value, buf, width as usize) as u32
}

/// The vector of tiles the MOD page draws for the LFO wave `wave` (the unclamped wave argument of its cell), 0 when no loaded shape has that number; stores the unipolar byte at `flag`.
/// `vtable` is the vtable word of the stock tile vector's first `Bitmap`. What the graph hook (`hooks-ui.s`) calls. C calling convention.
///
/// # Safety
/// `flag` is valid for one byte.
#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_lfo_graph(wave: u32, vtable: u32, flag: *mut u8) -> *const u8 {
    dkrt_svc_lfo::graph::lookup(wave, vtable, flag)
}
