//! The generated core as a value.
//!
//! Everything the engine reads from the generated SHARC+ core (the tables of its image and the
//! functions of its transpiled interpreter) is one [`GenCore`]. `sharc-gen` builds the table from
//! the generated modules; the engine reaches the core only through `St::core`. [`GenCore::NONE`]
//! is the runtime without a generated core: no image, no blocks, and every instruction traps.
//!
//! The function fields are plain `fn` pointers, so a core can be linked statically or, later,
//! loaded from a library. Calls through them are indirect and not inlined; the per-instruction
//! interpreter (`exec_insn`) is one such call, with the generated core inlined behind it.

use crate::BlockFn;
use crate::rt::{
    Insn, Int, R, RT_SYMS, St, Sym, TRAP_INDEX, TRAP_NO_INSN, TRAP_RT_BASE, Trap, V, rt_trap_name,
    s_set_r,
};

/// One generated core: image tables, string tables and interpreter entry points. Copying the table
/// copies the references and function pointers, not the core.
#[derive(Clone, Copy)]
pub struct GenCore {
    /// A transpiled core is linked (false: the runtime fallbacks, every instruction traps).
    pub generated: bool,
    /// The core has an image (blocks, instruction table, code ranges) made for one DSP firmware.
    pub has_image: bool,
    /// Name of the DSP image the core was made for (`dn2-1.12`; `none` without an image).
    pub image: &'static str,
    /// SHA-256 of the DSP image (`none` without an image).
    pub image_sha256: &'static str,
    /// SHA-256 of the `sharc_core` sources the core was transpiled from (`none` without a core).
    pub core_sha256: &'static str,
    /// Generator version (0: generated before the version existed, or no core).
    pub generator_version: u32,
    /// The `explicit_memory_model` setting the image's block code was generated for.
    pub explicit_memory_model: bool,
    /// Suffix of the environment variables that hold this core's fast-tier address lists
    /// (`SHARC_FAST_REGIONS` plus the suffix): they name addresses of one device's image. It belongs to the
    /// host's slot for the device, not to the core: the host sets it on a loaded core
    /// (`sharc_native::core_lib::load`).
    pub env_suffix: &'static str,
    /// AOT blocks by entry PC.
    pub blocks: &'static [(u32, BlockFn)],
    /// Code ranges of the image: start, length in short words, SHA-256 of the words.
    pub code_ranges: &'static [(u32, u32, &'static str)],
    /// Full block bounds (entry, exclusive end), sorted by entry PC. An empty table
    /// also covers images generated before this metadata was available.
    pub block_bounds: &'static [(u32, u32)],
    /// Entry PCs per region, for barring blocks after a code change (None: generated without).
    pub region_entries: Option<&'static [&'static [u32]]>,
    /// Entry PCs of blocks that are safe with the peripheral models on.
    pub model_safe: &'static [u32],
    /// DO loop end addresses of the image (sorted).
    pub loop_ends: &'static [i64],
    /// The packed instruction table of the image.
    pub insn_blob: &'static [u8],
    /// Decoded instruction of the image at a PC.
    pub insn_at: fn(Int) -> Option<Insn>,
    /// Names of the generated trap sites.
    pub trap_sites: &'static [&'static str],
    /// The string table (symbol names).
    pub sym_names: &'static [&'static str],
    /// A name in the string table.
    pub sym_of: fn(&str) -> Option<Sym>,
    /// The core's register write.
    pub write_ureg: fn(&mut St, Int, V) -> R<()>,
    /// Execute one decoded instruction through the interpreter, undoing it if it traps.
    pub exec_insn: fn(&mut St, Insn) -> R<()>,
    /// `sequencer._advance`: loop bookkeeping and the PC after an instruction.
    pub advance: fn(&mut St, Insn) -> R<()>,
    /// `sequencer._interrupt_candidate`: the interrupt to take at this boundary (0: none).
    pub interrupt_candidate: fn(&mut St, Int) -> R<Int>,
    /// `sequencer._enter_interrupt`.
    pub enter_interrupt: fn(&mut St, Int) -> R<()>,
    /// `state._sync_pc_stack`.
    pub sync_pc_stack: fn(&mut St) -> R<()>,
    /// `state._sync_status_stack`.
    pub sync_status_stack: fn(&mut St) -> R<()>,
    /// `state._sync_empty_loop_registers`.
    pub sync_empty_loop_registers: fn(&mut St) -> R<()>,
    /// `state._pop_loop_resource`.
    pub pop_loop_resource: fn(&mut St) -> R<()>,
    /// `state._push_loop_resource`.
    pub push_loop_resource: fn(&mut St) -> R<()>,
    /// `state._pc_stack_depth`.
    pub pc_stack_depth: fn(&mut St) -> R<Int>,
    /// `state._pc_stack_top`.
    pub pc_stack_top: fn(&mut St) -> R<Int>,
    /// `state._push_pc_stack`.
    pub push_pc_stack: fn(&mut St, Int) -> R<()>,
    /// `sequencer._pop_pc_stack`.
    pub pop_pc_stack: fn(&mut St) -> R<()>,
    /// `state._packed_counter_laddr`.
    pub packed_counter_laddr: fn(&mut St, Int) -> R<V>,
}

fn none_insn_at(_pc: Int) -> Option<Insn> {
    None
}

fn none_sym_of(name: &str) -> Option<Sym> {
    RT_SYMS.iter().position(|&n| n == name).map(|i| i as Sym)
}

fn none_write_ureg(s: &mut St, code: Int, v: V) -> R<()> {
    if matches!(code, 100 | 101) {
        return Err(TRAP_INDEX);
    }
    s_set_r(s, code, v)
}

fn none_exec(_s: &mut St, _insn: Insn) -> R<()> {
    Err(TRAP_NO_INSN)
}

fn none_unit(_s: &mut St) -> R<()> {
    Err(TRAP_NO_INSN)
}

fn none_int(_s: &mut St) -> R<Int> {
    Err(TRAP_NO_INSN)
}

fn none_int_arg(_s: &mut St, _a: Int) -> R<Int> {
    Err(TRAP_NO_INSN)
}

fn none_unit_arg(_s: &mut St, _a: Int) -> R<()> {
    Err(TRAP_NO_INSN)
}

fn none_laddr(_s: &mut St, _a: Int) -> R<V> {
    Err(TRAP_NO_INSN)
}

impl GenCore {
    /// No generated core: the runtime fallbacks.
    pub const NONE: GenCore = GenCore {
        generated: false,
        has_image: false,
        image: "none",
        image_sha256: "none",
        core_sha256: "none",
        generator_version: 0,
        // The default configuration: a build without an image has no blocks whose model must match.
        explicit_memory_model: true,
        env_suffix: "",
        blocks: &[],
        code_ranges: &[],
        block_bounds: &[],
        region_entries: None,
        model_safe: &[],
        loop_ends: &[],
        insn_blob: &[],
        insn_at: none_insn_at,
        trap_sites: &[],
        sym_names: &RT_SYMS,
        sym_of: none_sym_of,
        write_ureg: none_write_ureg,
        exec_insn: none_exec,
        advance: none_exec,
        interrupt_candidate: none_int_arg,
        enter_interrupt: none_unit_arg,
        sync_pc_stack: none_unit,
        sync_status_stack: none_unit,
        sync_empty_loop_registers: none_unit,
        pop_loop_resource: none_unit,
        push_loop_resource: none_unit,
        pc_stack_depth: none_int,
        pc_stack_top: none_int,
        push_pc_stack: none_unit_arg,
        pop_pc_stack: none_unit,
        packed_counter_laddr: none_laddr,
    };

    /// Name of a trap: the generated site table or the runtime's own codes.
    pub fn trap_name(&self, t: Trap) -> String {
        if t.0 >= TRAP_RT_BASE {
            return rt_trap_name(t).to_string();
        }
        if let Some(site) = self.trap_sites.get(t.0 as usize) {
            return site.to_string();
        }
        format!("trap {}", t.0)
    }

    /// The string table's name for a symbol.
    pub fn sym_name(&self, s: Sym) -> &'static str {
        self.sym_names.get(s as usize).copied().unwrap_or("?")
    }
}
