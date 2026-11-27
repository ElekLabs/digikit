"""Database version, known code blocks, SQLite schema and database open/meta helpers."""

from __future__ import annotations

import os
import sqlite3
import subprocess

from tools.sharc_dev import sharcinv

# Names tools/sharc_dev (the parent of this package), as before the split.
_here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

DB_VERSION = 17

# Bump DB_VERSION whenever the schema or the semantics of an existing column
# change, so build_database()'s skip-rebuild check does
# the right thing on the next run.
# v17 indexes printable runs from the loader blob and records build inputs.
# v16 also resolves a 2b/2c collision whose short reading is clean once its
# own successors are width-resolved (sharc_disasm._resolved_reach_clean).
# v15 also retries unresolved 2b/2c collisions through a bounded later
# rejoin, so old aligned instruction rows must be decoded again.
#
# --- v13: decode from LoadedMemory, not a code block's own stream bytes ----
#
# docs/findings/05-sharc-isa-and-decoding.md's "One decode path" section had
# already found (with tools/sharc_dev/sharc_coverage.py's --compare-decode-at) that a
# `code` loader block's own stream payload is not always what the image the
# emulator actually boots from holds at that address: a LATER block in the
# same boot stream can overwrite some or all of an earlier block's declared
# range before the whole stream finishes loading (LoadedMemory's documented
# last-write-wins resolution). Both places that built the `insn`/`functions`
# table's underlying decode -- this file's own per-code-block loop below and
# tools/sharc_dev/sharcinv.py's analyze_block()/build_inventory() (which this file's
# _fill_database() also calls, via tools/sharc_dev/sharcfn/'s load_context(), for
# function boundaries and call/jump/return sites) -- read a code block's OWN
# stream bytes (data[payload_offset:payload_offset+payload_len]). Both now
# instead read the block's declared [target_address, target_address+
# byte_count) range through a tools/sharc_dev/sharcldr.py LoadedMemory built over the
# whole stream, matching tools/sharc_dev/sharc_core/sequencer.decode_at()'s own input
# (no database access, LoadedMemory only) -- one decode INPUT as well as one
# decode function, for the same reason the "One decode path" section unified
# the decode FUNCTION.
#
# Two concrete, cross-checked consequences (see the finding for the byte-
# level evidence and the second-agent check):
#
#  1. Loader block 1 (all four images, target 0x282403f0, byte_count 10312)
#     is dead in the final image: 10304 of its own 10312 declared bytes
#     (99.92%, identical fraction and identical two 4-byte survivor gaps on
#     all four images) are overwritten by later blocks in the same stream --
#     mostly one large zero FILL, the rest small `data` blocks -- before
#     LoadedMemory's resolution is reached. No root or IVT slot enters it;
#     the many `dataref_code_pointer` roots and ~82 "functions" the old
#     per-block scan found inside it were themselves artifacts of decoding
#     these dead bytes as if they were live code (self-referential immediate
#     values that happen to land back in the block's own dead address
#     range). tools/sharc_dev/sharcinv.py's CODE_BLOCKS (and this file's
#     KNOWN_CODE_BLOCKS entries for all four images) no longer include it;
#     its blocks-table row now reports `kind='data'` (or 'fill'), matching
#     what the final image actually uses that address range for -- not a
#     "code" block LoadedMemory happens to give a different read for, but
#     not a program-code block at all. This removes the six `26a`-vs-`21a`
#     stale-bytes "SYNC" coverage gaps sharc_coverage.py --all-roots
#     reported at sw 0x1208e4/0x1208ff/0x12096d/0x12099c/0x1209b5/0x1209c4.
#
#  2. DN2 1.11/1.10E's loader block 57 (target 0x2001e888, byte_count 67652)
#     is NOT overwritten at all -- byte-for-byte identical to LoadedMemory
#     across its whole declared range -- but tools/sharc_dev/sharcldr.py's L2_BYTE_LIMIT
#     (previously 0x20020000, one 128 KB L2 SRAM bank) was too small: the
#     ADSP-2156x hardware reference documents 1 MB of L2 SRAM across eight
#     128 KB banks, and block 57 itself extends to 0x2002f1c4, 61,900 bytes
#     (91.5%) past the old bound. read_sw()'s L2 fallback bounds-checked
#     against that too-small limit and returned None there, which is what
#     tools/sharc_dev/sharc_coverage.py's --compare-decode-at reported as "stale
#     bytes" even though nothing in the boot stream ever touches those
#     bytes again -- a mapping bug, not a real overwrite. L2_BYTE_LIMIT is
#     now 0x20100000 (the full 1 MB window); block 57 stays in
#     KNOWN_CODE_BLOCKS, now correctly resolved end to end.
#
# `blocks` gained an `overwritten_bytes` column (tools/sharc_dev/sharcldr.py's new
# LoadedMemory.owner_runs()): for every non-FILL block, the count of its own
# declared bytes NOT owned by itself in the final image (0 = fully intact).
# General diagnostic, not just for the two cases above.
#
# --- v12: Type3d/4d/14d confident decode; Type15a mem_access/dataref fix ----
#
# Two independent changes:
#
# 1. decode_table.json promotes Type3d, Type4d and Type14d from
#    unconfirmed_bits>0 ("uncertain" -- no second source beyond the single
#    PRM figure, docs/findings/05's "SHARC+-only encodings with no second
#    source" list) to unconfirmed_bits=0 ("confident"). Checked two ways,
#    independently, and recorded in docs/findings/05-sharc-isa-and-decoding.md:
#    (a) raw bytes of individual dt2-1.16 instances, re-extracted bit-by-bit
#    from the manual's own figure geometry (not through sharc_isa.py),
#    reproduce the database's decoded fields exactly, and the mnemonic
#    suffix the existing renderer already picks for every real Type14d
#    instance matches the PRM's own (l,x,d) -> BH/BHSE-family syntax table
#    with no exceptions; (b) the addresses/registers each instance touches
#    (repeated DM globals, a function-prologue register-save idiom, the
#    already-[V] 0x82a00000 SoC command block) are the dataflow a compiler
#    would actually emit, and every one of the 158 whole-image instances
#    (87 on the sharc_coverage.py render path from 0x1c2b24) decodes a
#    clean, aligned successor instruction. This removes the
#    "provisional 14d/4d/3d: decode confidence uncertain" sharc_coverage.py
#    gaps and lets sharc_trace.py/sharc_run.py execute these forms without
#    passing them as provisional.
#
# 2. extract_mem_access()'s DIRECT_MEM_FORMS branch treated Type15a's
#    32-bit "addr" field as an absolute address (mem_access.abs_address),
#    the same wrong assumption the v6 `ptr`-table comment already flagged
#    and _ptr_mem_form() already avoids: sharc-plus-prm pp.387-390 (Figure
#    15-3, "DM(24,I5)=TCOUNT") gives Type15a as DM(<data32>,Ia)/PM(<data32>,
#    Ic) -- an I-register-relative access, not absolute, unlike 14a/14d
#    (which this branch also handles and which genuinely are absolute).
#    429 dt2-1.16 rows carried a negative offset such as -0x87 (I7-relative)
#    as abs_address 0xffffff79 -- indistinguishable from a real address near
#    the top of the 32-bit space. Fixed: Type15a now gets its own branch in
#    extract_mem_access, setting base_reg/modifier (the same columns
#    INDEXED_MEM_FORMS/IMMOFF_MEM_FORMS use for their own register-relative
#    accesses) from the manual's g/i-bank and sign-extended offset, and
#    leaving abs_address NULL. The parallel dataref bug -- extract_literal's
#    caller tagged Type15a's literal role='abs_load'/'abs_store' the same
#    way as Type14a's -- is fixed the same way: Type15a's literal now falls
#    through to role='literal' (a modify delta, like 16a/18a/19a*), not an
#    address. INDEXED_MEM_FORMS/IMMOFF_MEM_FORMS (3a/3b/3d/6a_mem/4a/4b/4d/
#    15b) were checked and never set abs_address in the first place: only
#    Type15a's presence in the DIRECT_MEM_FORMS grouping caused this.
#
# --- v11: Type14d/3b/3d/3d(ptr) mem_access/ptr width -------------------------
#
# The same class of bug v10 fixed for Type4b/4d: extract_mem_access()'s
# DIRECT_MEM_FORMS branch still used the generic "long if l else word"
# fallback for Type14d (wrong -- its l/x bits pick byte/short(-sign-
# extended), see _type14d_width()); its INDEXED_MEM_FORMS branch always
# recorded width=None for Type3b/3d instead of decoding their l/x/w (3b)
# or w/ex/l/x (3d) bits (see _type3d_width()); and _ptr_mem_form()'s own
# IMMOFF_MEM_FORMS branch had the same "long if l else word" mislabeling
# of Type4b/4d bug v10 fixed in extract_mem_access, now fixed by reusing
# _immoff_width() there too (width is descriptive only in `ptr` rows --
# address arithmetic is untouched).
#
# --- v10: MULT regdef/reguse naming, Type4b/4d mem_access width ------------
#
# _compute_regdef_reguse()'s MULT branch used classify_compute()'s is_float,
# which is wrong for every MULT opcode except 0x30 ("FN = FX*FY"): bit 3 of
# the mulop opcode is MOD1's Integer/Fraction result-format bit, not a
# float/fixed switch (PRM Table 18-7). regdef/reguse rows for other MULT
# opcodes (e.g. 0x48, "UUF"-style fixed-point multiplies) previously named
# F-registers where the manual calls for R-registers.
#
# extract_mem_access() also mislabeled Type4b/4d's l/x/w-encoded byte (bw)
# and short-word (sw) accesses as "word"/"long" (that fallback is only
# correct for forms whose lone "l" bit really means long-word); see
# _immoff_width() and sharc_trace.ACCESS_WIDTHS.
#
# --- v7: interrupt-vector roots ---------------------------------------------
#
# CMMR_SYSCTL.IIVT is set at boot (confirmed by the byte-exact slot targets
# below), so the IVT lives in L1 block 0's 48-bit (PM) space at the fixed
# short-word address IVT_SW -- sc58x-2158x-prm pp.226-231 ("L1 Memory
# Interface": "Block 0 has 256 addresses reserved for internal interrupt
# vector table"; the exact numeric address isn't in either public manual,
# "see the memory-map in the product specific data sheet") and
# sharc-plus-prm Table 4-47 (IIVT=1 -> L1) / Table 4-46 (the 32 interrupt
# names IVT_VECTOR_NAMES below reuses verbatim). _detect_interrupt_vector_roots()
# reads the 32x24-byte slot table there and adds a `roots` row per slot whose
# 4 instructions aren't the all-empty pattern, for every image (DT2 and DN2
# alike -- the location and slot layout were checked byte-for-byte identical
# across all four .sqlite images this repo builds). Two further ISR dispatch
# tables (IVT_DISPATCH_TABLES, an ASTATX-indexed and a SEC-id-indexed one)
# were checked the same way and found to sit entirely inside a boot-stream
# FILL block on DT2 1.16/1.15C (argument=0, i.e. the ROM ships them zeroed) --
# _detect_dispatch_table_roots() detects that case and adds no root rather
# than reporting an all-zero table as "no code pointers" by coincidence.
#
# --- v6: `ptr` -- constant-pointer propagation ------------------------------
#
# mem_access.abs_address (and dataref's resolved_offset) only ever resolved a
# direct absolute form or a same-block literal-then-immediate-offset pair;
# every other computed address -- a literal loaded into an I register three
# blocks earlier, a post-modify walk by a hardware-reset M constant, a
# spilled/reloaded call argument, a DM(I6+k) frame slot -- stayed invisible
# to "who writes address X". analyze_image() now also runs a small,
# deterministic forward constant-propagation per function (see
# _build_ptr_rows()'s own module note, just above it) and records every
# memory access it can pin to a known base register value or a fully known
# address in the new `ptr(image, sw, base_reg, base_value, address,
# direction, width)` table; tools/sharc_dev/sharc.py's Image.writers()/readers() query
# it. analyze()-only (no re-decode) is enough to backfill this on an
# existing database -- see analyze_image()'s own CREATE TABLE IF NOT EXISTS.
#
# --- v5: reach now seeds from every root, not just function entries --------
#
# _detect_reach() used to drop a root whose address wasn't inside any
# function's [entry_sw, end_sw) span. The only root kind that can happen to
# (loader_entry -- see _detect_roots) was therefore silently missing from
# `reach` on every image: DT2 1.16 and DN2 1.11 both put their loader entry
# inside a `data` block immediately followed by `fill` padding, with no
# bblock/succ path bridging that padding into the first application
# function (checked directly against both images' bblocks/succ rows). See
# _detect_reach()'s own docstring for the fallback this version adds.
#
# --- ColdFire images (import_ghidra) -----------------------------------------
#
# build_ghidra_database() fills this SAME schema from a tools/coldfire/ghidradump.py
# dump directory (manifest.json/xrefs.sqlite/disasm/decomp) instead of
# decoding a SHARC+ blob, so every image -- SHARC or ColdFire -- is one
# `sharcdb.load()`/`sharc.py` API. meta.cpu is 'sharc' or 'coldfire';
# sharc.Image.trace() checks it and raises for a ColdFire image (no SHARC
# symbolic tracer for m68k). Column mapping, one column at a time:
#   - insn.sw / functions.entry_sw,end_sw / edges.from_sw,to_sw / dataref.sw:
#     the ColdFire BYTE address (there is no "short word" unit on this CPU;
#     the column is reused as-is rather than adding a parallel `addr` column
#     everywhere it's already keyed on `sw`).
#   - insn.raw: hex bytes exactly as tools/coldfire/ghidradump.py's disasm_line() wrote
#     them (concatenated 16-bit words, no spaces); insn.mnemonic: its
#     rendered text (Ghidra's CodeUnitFormat string, not ours); insn.block,
#     .form, .fields, .cond, .confidence: NULL (no per-instruction decode of
#     our own to fill them). insn.aligned is always 1 (Ghidra has already
#     disassembled every byte; there is no sweep/depth ambiguity to record).
#   - functions.block/.label/.entry_kind: NULL (no loader block, no SHARC
#     label pass); functions.has_static_caller/.is_leaf: from the `edges`
#     this same import wrote (any incoming/outgoing call edge), same
#     definition as the SHARC build.
#   - edges: one row per tools/coldfire/ghidradump.py `calls` row (already function-
#     level: a plain intra-function jump is dropped by that tool's own
#     collect(), never a reference at all). kind is the Ghidra reference type
#     with its UNCONDITIONAL_/COMPUTED_ prefix stripped down to this schema's
#     call/jump/cond_jump vocabulary (COMPUTED_* rounds down to the same kind
#     as its unconditional counterpart -- Ghidra had already resolved the
#     target, so there is no "unknown target" left the way SHARC's own
#     'indirect' kind means); the ORIGINAL Ghidra kind string (e.g.
#     "COMPUTED_CALL") is kept in `note` so that distinction isn't lost.
#     `fallthrough`/`return`/`indirect` never appear: a Ghidra dump has no
#     basic-block CFG to derive them from (see `bblocks`/`succ` below).
#   - dataref: one row per tools/coldfire/ghidradump.py `data_refs` row; value=to_addr,
#     sw=site, form='ghidra', role=the Ghidra kind lowercased
#     (read/write/read_write/data) -- not a SHARC form label (14a/17a/...),
#     since nothing here decoded the instruction that owns it.
#   - decomp(image, function_sw, c): decompiled C read verbatim from
#     tools/coldfire/ghidradump.py's own decomp/*.c (empty for a SHARC image, which
#     has no decompiler in this pipeline: Ghidra-only table, PK (image,
#     function_sw)).
#   - blocks, literals, mem_access, regdef, reguse, func_hash, bblocks, succ,
#     idom, loops: left EMPTY for a ColdFire image -- known gaps, not
#     silently-wrong data:
#       * blocks: Ghidra's own memory blocks (ram/MCF5441X_* MMIO regions)
#         don't fit this table's loader-block shape (idx/target_address/
#         byte_count); query xrefs.sqlite's own `blocks` table instead.
#       * literals/mem_access/regdef/reguse: instruction-decode facts this
#         import never computes (no re-disassembly of the ColdFire bytes;
#         Ghidra's own P-code has the same information but importing it is
#         future work, not done here).
#       * func_hash: relocation-tolerant hashing needs a decoder's own field
#         layout (SHARC's mask_relocatable); no ColdFire equivalent here, so
#         no cross-image function matching for ColdFire images yet.
#       * bblocks/succ/idom/loops: NO per-instruction CFG survives a Ghidra
#         dump at all -- tools/coldfire/ghidradump.py's collect() only records a
#         reference that is a CALL or a JUMP to a DIFFERENT function's entry
#         (see its build_rows()); an ordinary intra-function branch is never
#         turned into a reference, so this import cannot reconstruct basic
#         blocks, successors, dominators or loops. `functions`/`edges`
#         (function-level call graph) are still complete; analyze_image()'s
#         idom/loop pass simply sees no bblocks rows for this image and
#         produces no rows for it either (not an error).
#   - roots: the SHARC-only detectors (loader_entry, code_pointer_array,
#     rtos_task) are already gated on `mem`/`blocks`/the `literals` table,
#     which build_ghidra_database() never supplies, so they emit nothing for
#     a ColdFire image without a separate code path. dataref_code_pointer and
#     no_static_entry are already CPU-generic (pure SQL over
#     dataref/edges/functions) and fire the same way for both CPUs. The one
#     ColdFire-specific ADDITION is `vector_entry`: any function whose Ghidra
#     name matches `vector_<N>_handler` (an exception/interrupt vector table
#     entry Ghidra already named) -- a SHARC function is always named
#     FUN_%06x, so this pattern never matches there either, and needed no
#     cpu flag threaded through _detect_roots.
#   - reach/callgraph: fully reused (they only read edges/functions, both
#     CPU-generic tables).

# --- known per-image code-block selections ----------------------------------
#
# See the module docstring for how each entry was derived. dt2-1.15C's block
# layout was verified identical to dt2-1.16's (same indices, same target
# addresses) by direct inspection of both blobs' parsed block lists.
KNOWN_CODE_BLOCKS = {
    # dt2-1.16
    "0f514a12a2255f5c081e292c47f1f29462003177658da4bbae0a22fd737fffa2": sharcinv.CODE_BLOCKS,
    # dt2-1.15C (identical block layout to 1.16 -- verified by inspection)
    "6d4316cddd41edef7a136c10d270313882028a59b96cc8fe97a716b949a7d551": sharcinv.CODE_BLOCKS,
    # dn2-1.11: blk56 L2 main(0x20000000), blk57 L2 extension(0x2001e888,
    # within L2_BYTE_LIMIT), blk66/68/70 the three small
    # 0x2838004c/0x283801a4/0x28380328 blocks, blk78 startup (0x28380548),
    # blk81 the small 0x2838261c block, blk83 the main code region
    # (0x28382720). blk1 (target 0x282403f0, same as DT2's dead block 1) is
    # left out -- see the module docstring. blk59 (0x200779f8) is also left
    # out: the v13 L2_BYTE_LIMIT widening now covers its start address, but
    # it has no prior characterization as code (unlike blk57).
    "336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2": (
        56,
        57,
        66,
        68,
        70,
        78,
        81,
        83,
    ),
    # dn2-1.10E: same target-address correspondence, shifted indices.
    "174b391822bbe33a99e5f42bd02ef3ea75c0e79351caae6ba90e4cd2c4de5350": (
        56,
        57,
        67,
        69,
        71,
        79,
        82,
        84,
    ),
}

SCHEMA = """
CREATE TABLE meta (image TEXT, key TEXT, value TEXT, PRIMARY KEY (image, key));

CREATE TABLE blocks (
    image TEXT, idx INTEGER, target_address INTEGER, byte_count INTEGER,
    kind TEXT,              -- 'code' | 'data' | 'fill'
    base_sw INTEGER,        -- NULL when no known addressing convention covers it
    -- DB_VERSION v13: bytes of this block's own declared range NOT owned by
    -- itself in the final image (LoadedMemory.owner_runs(), last write
    -- wins) -- 0 means every byte this block wrote is still there; NULL for
    -- FILL blocks (a FILL block contributes no bytes of its own to compare).
    overwritten_bytes INTEGER,
    PRIMARY KEY (image, idx));

-- Every offset tools/sharc_dev/sharcimm.py's linear sweep decodes in a scanned code
-- block (tools/sharc_dev/sharcpcode.py's 'decoder' table does the same for one main
-- program; this does it per code block, for every scanned block). width is
-- bytes; mnemonic is only rendered for aligned rows (see module docstring).
CREATE TABLE insn (
    image TEXT, sw INTEGER, block INTEGER, width INTEGER, raw TEXT, form TEXT,
    fields TEXT, cond INTEGER, mnemonic TEXT, aligned INTEGER, confidence TEXT,
    function_sw INTEGER,
    PRIMARY KEY (image, sw));

CREATE TABLE functions (
    image TEXT, entry_sw INTEGER, end_sw INTEGER, n_insns INTEGER, block INTEGER,
    name TEXT, has_static_caller INTEGER, is_leaf INTEGER, label TEXT, entry_kind TEXT,
    PRIMARY KEY (image, entry_sw));

-- kind: call, jump, cond_jump, fallthrough (the not-taken path of a
-- cond_jump), return, indirect (to_sw NULL; note carries the register).
CREATE TABLE edges (
    image TEXT, from_sw INTEGER, to_sw INTEGER, kind TEXT, delayed INTEGER, cond INTEGER,
    from_function INTEGER, to_function INTEGER, note TEXT);

-- Printable runs in loaded bytes. Some may be coincidental instruction bytes.
CREATE TABLE strings (
    image TEXT, byte_addr INTEGER, sw_start INTEGER, sw_end INTEGER, block INTEGER, text TEXT,
    PRIMARY KEY (image, byte_addr)
);

CREATE TABLE literals (
    image TEXT, sw INTEGER, value INTEGER, form TEXT, dest_reg TEXT,
    in_code_range INTEGER, in_data_range INTEGER);

CREATE TABLE mem_access (
    image TEXT, sw INTEGER, space TEXT, direction TEXT, base_reg TEXT, modifier TEXT,
    u INTEGER, form TEXT, width TEXT, abs_address INTEGER);

CREATE TABLE func_hash (
    image TEXT, entry_sw INTEGER, exact_hash TEXT, reloc_hash TEXT, n_insns INTEGER, byte_len INTEGER,
    PRIMARY KEY (image, entry_sw));

-- Decompiled C per function, verbatim from tools/coldfire/ghidradump.py's decomp/*.c
-- (see the DB_VERSION comment's ColdFire-images section above). Empty for a
-- SHARC image -- no decompiler in this pipeline.
CREATE TABLE decomp (image TEXT, function_sw INTEGER, c TEXT, PRIMARY KEY (image, function_sw));

-- Basic blocks: a maximal straight-line run of aligned instructions, ended
-- by any branch/call/return/indirect site (after its delay slots, when
-- delayed -- see _gather_terminators) or by a hardware DO..UNTIL loop's
-- last body instruction (a terminator despite not being a branch itself),
-- and started by a function entry, any edge's target, or the instruction
-- after such a terminator. end_sw is exclusive (one past the last member
-- instruction), the same convention functions.end_sw already uses. block is
-- the loader/vendor-memory block index (blocks.idx) it was found scanning
-- -- unrelated to "basic block".
CREATE TABLE bblocks (
    image TEXT, start_sw INTEGER, end_sw INTEGER, function_sw INTEGER, n_insns INTEGER,
    block INTEGER,
    PRIMARY KEY (image, start_sw));

-- Basic-block control-flow successors. kind: fallthrough (plain, non-branch
-- block-to-block continuation -- present so a recursive CTE can walk
-- straight-line code, not just branch sites), jump, cond_taken,
-- cond_not_taken, call_return (the block after a call's delay slots --
-- the call edge itself, into the callee, stays in `edges`), loop_back (a
-- hardware DO..UNTIL's body-end -> body-start back edge), loop_exit,
-- return, indirect. to_block is NULL for return/indirect (no static
-- target) and for any edge whose target this pass never turned into a
-- bblocks row (a cross-image or unscanned-region jump); such a row is
-- still recorded so a "does every block exit account for its kind" audit
-- doesn't have to fall back to `edges`.
CREATE TABLE succ (
    image TEXT, from_block INTEGER, to_block INTEGER, kind TEXT);

-- Every constant an instruction materialises as an address: a 17a/17b/16a/
-- 16b/18a/19a*/15a/14a literal (tools/sharc_dev/sharcdb/'s own `literals` table,
-- given a role here) plus any 14a/14d absolute mem_access address (kept
-- separate from `literals` since 14d isn't a LITERAL_FORMS entry). role:
-- i_reg_base (a literal loaded directly into an I register -- a PM/DM table
-- base), abs_load/abs_store (a direct absolute address, from mem_access's
-- own direction so this never inherits a mnemonic-only bug -- see
-- tools/sharc_dev/sharcfn/'s Type3c note; 14a only -- see extract_mem_access's
-- module note on why 15a is not a direct-address form despite looking like
-- one), literal (any other literal-carrying form: 16a/16b, 18a, 17a/17b
-- into a non-I register, 19a* -- a modify delta, not an address; also 15a,
-- an I-register-relative offset -- see extract_mem_access), resolved_offset
-- (an IMMOFF form's I+immediate address, resolved when the I register was
-- just given a literal by a 17a/17b in the SAME basic block -- see
-- bblocks/succ above).
CREATE TABLE dataref (
    image TEXT, value INTEGER, sw INTEGER, form TEXT, role TEXT);

-- Register definitions and uses, from the typed decode fields (not from
-- mnemonic text -- see tools/sharc_dev/sharcfn/'s Type3c direction bug in the task
-- report). kind: compute (an ALU/MULT/SHIFT/MULTIFN/short-compute result),
-- mem_load (a data register loaded from memory), literal (a ureg set
-- directly from an immediate, or a hardware-loop LCNTR literal), move (a
-- 5a/5b_move ureg-to-ureg transfer, or a 12a_ureg loop count source), swap
-- (5a/5b_swap), dag_modify (an I register written by address-modify
-- semantics: 19a*'s Is XOR Idis dest, 7a's Is XOR Idis dest with an
-- M-register delta, 16a/16b's mandatory I+=M, or a 3a/3b/3d/4a/4b/4d
-- access with u=1), unknown (a register-touching form
-- family this pass could not fully resolve -- see the build report's
-- per-form unknown counts; never silently omitted).
CREATE TABLE regdef (
    image TEXT, sw INTEGER, reg TEXT, kind TEXT);

CREATE TABLE reguse (
    image TEXT, sw INTEGER, reg TEXT);

CREATE INDEX insn_function ON insn(image, function_sw);
CREATE INDEX insn_form ON insn(image, form);
CREATE INDEX edges_to ON edges(image, to_sw);
CREATE INDEX edges_from ON edges(image, from_sw);
CREATE INDEX edges_to_function ON edges(image, to_function);
CREATE INDEX edges_from_function ON edges(image, from_function);
CREATE INDEX literals_value ON literals(image, value);
CREATE INDEX mem_access_sw ON mem_access(image, sw);
CREATE INDEX mem_access_abs ON mem_access(image, abs_address);
CREATE INDEX func_hash_exact ON func_hash(exact_hash);
CREATE INDEX func_hash_reloc ON func_hash(reloc_hash);
CREATE INDEX functions_block ON functions(image, block);
CREATE INDEX bblocks_function ON bblocks(image, function_sw);
CREATE INDEX bblocks_end ON bblocks(image, end_sw);
CREATE INDEX succ_from ON succ(image, from_block);
CREATE INDEX succ_to ON succ(image, to_block);
CREATE INDEX dataref_value ON dataref(image, value);
CREATE INDEX dataref_sw ON dataref(image, sw);
CREATE INDEX regdef_reg ON regdef(image, reg);
CREATE INDEX regdef_sw ON regdef(image, sw);
CREATE INDEX reguse_reg ON reguse(image, reg);
CREATE INDEX reguse_sw ON reguse(image, sw);

-- --- analysis tables (tools/sharc_dev/sharcdb/'s analyze_image(), DB_VERSION 3) -----
--
-- Built once per image from edges/succ/dataref/literals with networkx,
-- reusing nothing decoded here: see analyze_image()'s docstring for the
-- detection heuristics (a root's kind explains how it was found).

-- kind: loader_entry (the scanned code region's own BFLAG_FIRST target),
-- rtos_task (an R4 value passed to a call site shaped like the R12/R8/R4
-- task-create idiom -- note carries the helper function and call site),
-- dataref_code_pointer (a literal/table value from `dataref` that lands
-- inside a scanned function), code_pointer_array (one resolved entry of a
-- consecutive in-code-pointer run found at an i_reg_base table address --
-- note carries the table base and index), no_static_entry (a function with
-- no edges of any kind pointing at its entry -- the same set `unentered`
-- clusters), interrupt_vector (SHARC only: one populated slot of the L1
-- hardware IVT at IVT_SW -- note carries 'slot N NAME', NAME from
-- IVT_VECTOR_NAMES/sharc-plus-prm Table 4-46; see the DB_VERSION v7 comment
-- above), vector_entry (ColdFire only: a function Ghidra itself named
-- vector_<N>_handler -- an exception/interrupt vector table entry; note
-- carries that name).
CREATE TABLE roots (image TEXT, sw INTEGER, kind TEXT, note TEXT);
CREATE INDEX roots_image ON roots(image);

-- Functions reachable from each root through call/jump/cond_jump edges
-- (function-level, from the `edges` table's own from_function/to_function).
-- depth counts CALL edges only -- a tail/cross-function JUMP costs nothing,
-- so this is shortest CALL depth, not instruction distance.
CREATE TABLE reach (
    image TEXT, root_sw INTEGER, function_sw INTEGER, depth INTEGER,
    PRIMARY KEY (image, root_sw, function_sw));
CREATE INDEX reach_function ON reach(image, function_sw);

-- Call-graph closure per function (CALL edges only). is_recursive is a
-- self-loop or membership in a call-graph SCC of size > 1.
CREATE TABLE callgraph (
    image TEXT, function_sw INTEGER, n_callers INTEGER, n_callees INTEGER,
    n_transitive_callees INTEGER, max_depth INTEGER, is_recursive INTEGER,
    PRIMARY KEY (image, function_sw));

-- Immediate dominators of a function's own basic-block CFG (bblocks/succ
-- restricted to that function), from networkx.immediate_dominators. The
-- entry block itself has no row (its idom is conventionally itself).
CREATE TABLE idom (
    image TEXT, function_sw INTEGER, block INTEGER, idom_block INTEGER,
    PRIMARY KEY (image, function_sw, block));

-- Natural loops from back edges (succ edge whose target dominates its
-- source) plus hardware DO..UNTIL loops (succ's own loop_back kind), merged
-- per header when several back edges share one. depth is loop nesting by
-- body containment (1 = outermost).
CREATE TABLE loops (
    image TEXT, function_sw INTEGER, header_block INTEGER, n_blocks INTEGER,
    kind TEXT, depth INTEGER,
    PRIMARY KEY (image, function_sw, header_block));

-- Functions with no static entry (the roots.kind='no_static_entry' set),
-- grouped by the likely dispatcher: the code-pointer array/literal that
-- names them as a target, else the nearest preceding function with an
-- indirect site, else 'none'.
CREATE TABLE unentered (
    image TEXT, function_sw INTEGER, cluster TEXT,
    PRIMARY KEY (image, function_sw));

-- Constant-pointer propagation results (_build_ptr_rows(), DB_VERSION 6):
-- every memory access whose effective address this pass's forward constant
-- propagation resolves to a known base I register value or a fully known
-- address. base_reg/base_value are NULL/NULL for a pure absolute 14a/14d
-- access (no base register at all); address is NULL when only the base is
-- known (e.g. an indexed I,M form whose M modifier isn't a tracked
-- constant) -- tools/sharc_dev/sharc.py's Image.writers()/readers() report that case
-- separately ('base_only') rather than silently dropping it.
CREATE TABLE ptr (
    image TEXT, sw INTEGER, base_reg TEXT, base_value INTEGER, address INTEGER,
    direction TEXT, width TEXT);
CREATE INDEX ptr_address ON ptr(image, address);
CREATE INDEX ptr_base_value ON ptr(image, base_value);
CREATE INDEX ptr_sw ON ptr(image, sw);
"""


def git_commit():
    try:
        return (
            subprocess.check_output(
                ["git", "rev-parse", "--short", "HEAD"],
                cwd=_here,
                stderr=subprocess.DEVNULL,
            )
            .decode()
            .strip()
        )
    except Exception:
        return None


def open_db(path):
    out_dir = os.path.dirname(os.path.abspath(path))
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    db.executescript(SCHEMA)
    return db


def read_meta(path):
    """-> {key: value} from an existing DB's meta table, or {} if the file is
    absent, empty, or not a sharcdb database."""
    if not os.path.exists(path):
        return {}
    try:
        db = sqlite3.connect(path)
        try:
            return dict(db.execute("SELECT key, value FROM meta").fetchall())
        finally:
            db.close()
    except sqlite3.DatabaseError:
        return {}
