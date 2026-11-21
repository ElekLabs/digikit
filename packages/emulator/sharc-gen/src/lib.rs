//! The generated native SHARC+ core, apart from the engine so that editing
//! the engine or the fast tier does not recompile it.
//!
//! `tools/sharc_dev/sharc_transpile.py` and `tools/sharc_dev/sharc_rsgen.py` write the code
//! under `out/` (the block part embeds firmware); build with
//! `SHARC_GEN_DIR=<that directory>` to include it. Without it this crate
//! holds only the runtime fallbacks.
//!
//! The generated code names its runtime as `crate::rt::*`, `crate::canon::`,
//! `crate::EXIT_NEXT` and so on, as it did when everything was one crate.
//! `sharc-rt` provides those paths below. Two boundary functions need
//! generated data, so this crate wraps them: `decode_at` (the string table)
//! and `_load_normal_ureg` (the core's `_write_ureg`).
//!
//! [`CORE`] is the table the engine reads (`sharc_rt::GenCore`). `exec_insn` and the other entry
//! points are defined here, next to the generated code, so that the generated `#[inline]`
//! functions stay inlinable; the engine calls them through the table.

pub use sharc_rt::*;

use sharc_rt::rt::Sym;
#[cfg(not(sharc_gen))]
use sharc_rt::rt::{RT_SYMS, TRAP_INDEX, s_set_r};

/// The runtime under the paths the generated code uses, with the two
/// boundary functions that need generated data replaced.
pub mod rt {
    pub use sharc_rt::rt::*;

    pub mod bnd {
        use super::*;
        pub use sharc_rt::rt::bnd::*;

        /// sequencer.decode_at (the table is the image's, or the decoder's).
        #[inline(always)]
        pub fn decode_at(s: &mut St, _data: (), _base_sw: Option<Int>, pc_sw: Int) -> R<Insn> {
            decode_at_with(s, pc_sw, crate::sym_of)
        }

        /// memory._load_normal_ureg.
        #[inline(always)]
        pub fn _load_normal_ureg(s: &mut St, space: Sym, address: VI, code: Int) -> R<Option<V>> {
            _load_normal_ureg_with(s, space, address, code, crate::write_ureg)
        }
    }
}

/// The core's register write (without generated code: the runtime's).
#[inline(always)]
fn write_ureg(s: &mut St, code: Int, v: V) -> R<()> {
    #[cfg(sharc_gen)]
    {
        generated::core_g::state::_write_ureg(s, code, v)
    }
    #[cfg(not(sharc_gen))]
    {
        if matches!(code, 100 | 101) {
            return Err(TRAP_INDEX);
        }
        s_set_r(s, code, v)
    }
}

use sharc_rt::rt::{Insn, Int, R, St, V};

/// The string table's name for a symbol.
#[cfg(sharc_gen)]
pub fn sym_name(s: Sym) -> &'static str {
    generated::syms::SYM_NAMES
        .get(s as usize)
        .copied()
        .unwrap_or("?")
}

#[cfg(not(sharc_gen))]
pub fn sym_name(s: Sym) -> &'static str {
    RT_SYMS.get(s as usize).copied().unwrap_or("?")
}

/// Intern NAME into the generated string table (None if it is unknown).
pub fn sym_of(name: &str) -> Option<Sym> {
    #[cfg(sharc_gen)]
    {
        generated::syms::SYM_NAMES
            .iter()
            .position(|&n| n == name)
            .map(|i| i as Sym)
    }
    #[cfg(not(sharc_gen))]
    {
        RT_SYMS.iter().position(|&n| n == name).map(|i| i as Sym)
    }
}

/// The instruction table paths the generated image uses.
pub mod canon {
    pub use sharc_rt::insn_table::{InsnTable, insn_table};
}

#[cfg(sharc_gen)]
#[allow(
    clippy::all,
    unused,
    non_snake_case,
    non_upper_case_globals,
    unreachable_code
)]
pub mod generated {
    pub mod syms {
        include!(concat!(env!("SHARC_GEN_DIR"), "/syms.rs"));
    }
    pub mod tables {
        include!(concat!(env!("SHARC_GEN_DIR"), "/tables.rs"));
    }
    /// The core with every function forced inline (block code).
    pub mod core_i {
        include!(concat!(env!("SHARC_GEN_DIR"), "/core_i.rs"));
    }
    /// The core with normal inlining (the one-instruction interpreter).
    pub mod core_g {
        include!(concat!(env!("SHARC_GEN_DIR"), "/core_g.rs"));
    }
    #[cfg(sharc_image)]
    pub mod image {
        include!(concat!(env!("SHARC_GEN_DIR"), "/image.rs"));
    }
}

/// Execute one decoded instruction through the generated core, undoing it
/// if it traps.
#[inline(never)]
pub fn exec_insn(s: &mut St, insn: Insn) -> R<()> {
    #[cfg(sharc_gen)]
    {
        if s.cfg.core_timer {
            s.timer_written = false;
        }
        s.begin();
        match generated::core_g::forms::_execute(s, insn) {
            Ok(()) => {
                if s.probe.is_some() {
                    s.probe_note();
                }
                s.commit()
            }
            Err(t) => {
                s.rollback();
                return Err(t);
            }
        }
        if s.cfg.core_timer {
            // Peripheral time advances after a completed instruction. A
            // failed timer event rolls back itself, retaining that instruction.
            s.begin();
            match generated::core_g::sequencer::_core_timer_tick(s) {
                Ok(_) => s.commit_host(),
                Err(t) => {
                    s.rollback();
                    return Err(t);
                }
            }
        }
        Ok(())
    }
    #[cfg(not(sharc_gen))]
    {
        let _ = (s, insn);
        Err(sharc_rt::rt::TRAP_NO_INSN)
    }
}

/// The generated core as a table (without a generated core: `GenCore::NONE`).
#[cfg(not(sharc_gen))]
pub static CORE: GenCore = GenCore::NONE;

/// The generated core as a table.
#[cfg(sharc_gen)]
pub static CORE: GenCore = GenCore {
    generated: true,
    has_image: cfg!(sharc_image),
    image: image_name(),
    image_sha256: image_sha256(),
    core_sha256: generated::tables::CORE_SHA256,
    generator_version: generator_version(),
    explicit_memory_model: explicit_memory_model(),
    env_suffix: env!("SHARC_GEN_ENV_SUFFIX"),
    blocks: image_blocks(),
    code_ranges: image_code_ranges(),
    block_bounds: image_block_bounds(),
    region_entries: image_region_entries(),
    model_safe: image_model_safe(),
    loop_ends: image_loop_ends(),
    insn_blob: image_insn_blob(),
    insn_at: image_insn_at(),
    trap_sites: &generated::tables::TRAP_SITES,
    sym_names: &generated::syms::SYM_NAMES,
    sym_of,
    write_ureg,
    exec_insn,
    advance: generated::core_g::sequencer::_advance,
    interrupt_candidate: generated::core_g::sequencer::_interrupt_candidate,
    enter_interrupt: generated::core_g::sequencer::_enter_interrupt,
    sync_pc_stack: generated::core_g::state::_sync_pc_stack,
    sync_status_stack: generated::core_g::state::_sync_status_stack,
    sync_empty_loop_registers: generated::core_g::state::_sync_empty_loop_registers,
    pop_loop_resource: generated::core_g::state::_pop_loop_resource,
    push_loop_resource: generated::core_g::state::_push_loop_resource,
    pc_stack_depth: generated::core_g::state::_pc_stack_depth,
    pc_stack_top: generated::core_g::state::_pc_stack_top,
    push_pc_stack: generated::core_g::state::_push_pc_stack,
    pop_pc_stack: generated::core_g::sequencer::_pop_pc_stack,
    packed_counter_laddr: generated::core_g::state::_packed_counter_laddr,
};

#[cfg(sharc_gen)]
const fn image_name() -> &'static str {
    #[cfg(sharc_image)]
    {
        generated::image::IMAGE
    }
    #[cfg(not(sharc_image))]
    {
        "none"
    }
}

#[cfg(sharc_gen)]
const fn image_sha256() -> &'static str {
    #[cfg(sharc_image)]
    {
        generated::image::IMAGE_SHA256
    }
    #[cfg(not(sharc_image))]
    {
        "none"
    }
}

#[cfg(sharc_gen)]
const fn generator_version() -> u32 {
    #[cfg(sharc_gen_version)]
    {
        generated::tables::GENERATOR_VERSION
    }
    #[cfg(not(sharc_gen_version))]
    {
        0
    }
}

/// The explicit_memory_model setting the image's block code was generated
/// for (true without an image: the default configuration).
#[cfg(sharc_gen)]
const fn explicit_memory_model() -> bool {
    #[cfg(sharc_image)]
    {
        generated::image::GEN_EXPLICIT_MEMORY_MODEL
    }
    #[cfg(not(sharc_image))]
    {
        true
    }
}

#[cfg(sharc_gen)]
const fn image_blocks() -> &'static [(u32, sharc_rt::BlockFn)] {
    #[cfg(sharc_image)]
    {
        generated::image::BLOCKS
    }
    #[cfg(not(sharc_image))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_code_ranges() -> &'static [(u32, u32, &'static str)] {
    #[cfg(sharc_image)]
    {
        generated::image::CODE_RANGES
    }
    #[cfg(not(sharc_image))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_block_bounds() -> &'static [(u32, u32)] {
    #[cfg(all(sharc_image, sharc_block_bounds))]
    {
        generated::image::BLOCK_BOUNDS
    }
    #[cfg(not(all(sharc_image, sharc_block_bounds)))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_region_entries() -> Option<&'static [&'static [u32]]> {
    #[cfg(all(sharc_image, sharc_region_entries))]
    {
        Some(generated::image::REGION_ENTRIES)
    }
    #[cfg(not(all(sharc_image, sharc_region_entries)))]
    {
        None
    }
}

#[cfg(sharc_gen)]
const fn image_model_safe() -> &'static [u32] {
    #[cfg(sharc_image)]
    {
        generated::image::MODEL_SAFE
    }
    #[cfg(not(sharc_image))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_loop_ends() -> &'static [i64] {
    #[cfg(sharc_image)]
    {
        generated::image::LOOP_ENDS
    }
    #[cfg(not(sharc_image))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_insn_blob() -> &'static [u8] {
    #[cfg(sharc_image)]
    {
        generated::image::INSN_BLOB
    }
    #[cfg(not(sharc_image))]
    {
        &[]
    }
}

#[cfg(sharc_gen)]
const fn image_insn_at() -> fn(Int) -> Option<Insn> {
    #[cfg(sharc_image)]
    {
        generated::image::insn_at
    }
    #[cfg(not(sharc_image))]
    {
        fn none(_pc: Int) -> Option<Insn> {
            None
        }
        none
    }
}
