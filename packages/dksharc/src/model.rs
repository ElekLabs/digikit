//! The result of the analysis, as serde types. Every address is a short-word pc.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallTarget {
    /// A function of this module at `pc`.
    Local { pc: u32 },
    /// A symbol the runtime resolves (a relocation of class 2 on the call): `import:<symbol>` in text form.
    Import { symbol: String },
    /// Through index and modify registers: the target is not known statically.
    Indirect { i: u8, m: u8 },
    /// A direct target outside the text of the module.
    Outside { pc: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub pc: u32,
    /// Index into `Analysis::functions`, None for a call outside every function.
    pub caller: Option<usize>,
    pub target: CallTarget,
    /// The call has delay slots.
    pub delayed: bool,
    /// The call is conditional.
    pub conditional: bool,
}

impl Call {
    /// `import:<symbol>` or the name of the local function, as the Python P3a notes wrote it.
    pub fn label(&self, a: &Analysis) -> String {
        match &self.target {
            CallTarget::Import { symbol } => format!("import:{symbol}"),
            CallTarget::Local { pc } => a
                .functions
                .iter()
                .find(|f| f.entry == *pc)
                .map_or_else(|| format!("{pc:#x}"), |f| f.name.clone()),
            CallTarget::Indirect { i, m } => format!("indirect:I{i},M{m}"),
            CallTarget::Outside { pc } => format!("outside:{pc:#x}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopCount {
    /// `DO ... UNTIL LCE` with a literal count.
    Lit { count: u32 },
    /// The count is loaded from a register (Ureg code of the instruction).
    Reg { ureg: u8, name: String },
    /// `DO ... UNTIL <condition>`: no count (`term` is the termination condition code).
    Until { term: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoLoop {
    /// pc of the DO instruction.
    pub do_pc: u32,
    /// First instruction of the body.
    pub start: u32,
    /// Last instruction of the body (the loop-back test is at its end).
    pub end: u32,
    pub count: LoopCount,
    /// Index of the innermost loop that contains this one.
    pub parent: Option<usize>,
    /// 0 for an outermost loop.
    pub depth: u32,
    pub function: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    Dm,
    Pm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Load,
    Store,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// `Space(Ii, Mm)`: the address is `Ii`, then `Ii += Mm`.
    PostModify,
    /// `Space(Mm, Ii)`: the address is `Ii + Mm`, `Ii` is kept.
    PreModify,
    /// `Space(<imm>, Ii)` with a post-modify by the immediate (`imm`).
    PostModifyImm,
    /// `Space(<imm>, Ii)`: the address is `Ii + imm` (Types 4 pre-modify, 15), `Ii` is kept.
    PreModifyImm,
    /// `Space(<addr>)`: an address in the instruction.
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Normal,
    /// The `(LW)` modifier: a register pair.
    Long,
    /// Byte or short word (the `l`, `x`, `w` bits select; not decoded here).
    SubWord,
}

/// One memory access of an instruction, as the form encodes it. Index registers are numbered I0-I15 and M0-M15.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemOp {
    pub space: Space,
    pub dir: Direction,
    pub mode: Mode,
    pub access: Access,
    pub i: Option<u8>,
    pub m: Option<u8>,
    /// The immediate of `PostModifyImm` and `PreModifyImm` (signed).
    pub imm: Option<i64>,
    /// The direct address of `Direct`.
    pub addr: Option<u32>,
    /// An import relocation sits on the address field.
    pub import: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Insn {
    pub pc: u32,
    /// Length in parcels (1 to 3).
    pub parcels: u8,
    /// The form name of the ISA table (`8a_abs`, `3a`, ...).
    pub form: String,
    /// Name of the import the relocation on this instruction refers to.
    pub import: Option<String>,
    pub mem: Vec<MemOp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Terminator {
    /// The block ends because the next instruction starts another block.
    Fall,
    Jump {
        target: Option<u32>,
        conditional: bool,
        delayed: bool,
    },
    /// `JUMP (Mm, Ii)`: the target is not known statically.
    IndirectJump {
        i: u8,
        m: u8,
        conditional: bool,
        delayed: bool,
    },
    Call {
        conditional: bool,
        delayed: bool,
    },
    Return {
        conditional: bool,
        delayed: bool,
    },
    /// A form that transfers control and that this analysis does not model (its edges are missing).
    Unmodelled {
        form: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Fall,
    Taken,
    /// The loop-back of a DO loop from the instruction at its end to its start.
    LoopBack,
    /// After a call returns.
    ReturnSite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub to: u32,
    pub kind: EdgeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub start: u32,
    /// pc of the instruction behind the block (delay slots included).
    pub end: u32,
    /// pc of the control transfer that ends the block (None for `Fall`).
    pub term_pc: Option<u32>,
    pub term: Terminator,
    /// pcs of the delay slot instructions that belong to the transfer, in order.
    pub delay_slots: Vec<u32>,
    pub succ: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub entry: u32,
    /// pc behind the last instruction (the next function's entry or the end of the text).
    pub end: u32,
    pub global: bool,
    /// Why it starts here: `symbol`, `entry:render`, `entry:note_on`, `entry:fx_process`, `call`, `text_start`.
    pub origin: Vec<String>,
    /// False when an instruction in the function did not decode with confidence, or a transfer is not modelled.
    pub complete: bool,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Undecoded {
    pub pc: u32,
    /// `unknown` or `uncertain`.
    pub decode: String,
    /// The form of an uncertain decode.
    pub form: Option<String>,
    /// The raw parcels (up to three) at `pc`.
    pub parcels: Vec<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub instructions: usize,
    pub functions: usize,
    pub blocks: usize,
    pub loops: usize,
    pub calls: usize,
    pub import_calls: usize,
    pub undecoded: usize,
    pub memory_operands: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Analysis {
    pub base_sw: u32,
    pub text_parcels: u32,
    pub summary: Summary,
    pub functions: Vec<Function>,
    pub loops: Vec<DoLoop>,
    pub calls: Vec<Call>,
    pub insns: Vec<Insn>,
    /// Decodes that are not confident: reported, not guessed.
    pub undecoded: Vec<Undecoded>,
    /// Structural findings: a branch into a delay slot, a loop that ends off an instruction, a symbol not on an instruction start.
    pub diagnostics: Vec<String>,
}
