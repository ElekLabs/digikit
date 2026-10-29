# DN2 cache configuration and LP0 staging

2026-10-30, DN2 OS 1.12. **[D]** static inspection and existing artifacts;
**[O]** production ownership/coherence unresolved; **[C]** corrected inference.
Original-byte checks were independently audited; no new hardware, emulator run,
core change or regeneration is part of this checkpoint. Section-7 SHA-256:
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
See the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Cache contract and current RX placement

**[D]** The public [SHARC sources](../sharc/SOURCES.md), Programming Reference
Rev 1.5 printed pp. 8-5, 8-8–12 and 35-7–8, and ADSP-2156x Hardware Reference
Rev 1.0 p. 9-9 describe 64-byte DM/PM cache lines. Cache cross-checking does not
establish DMA coherence. Dirty CPU output needs writeback before a DMA read;
DMA-written RX needs invalidation before a CPU read. Maintenance requires
exclusive ownership of the whole affected cache lines, including edge padding.

**[C]** Private 0.2 RX is module BSS in `dsp.l2`, not the L1 allocator:
[`lp0-proof.s`](../../modules/core/adapters/dsp-lp0-proof/src/dsp/lp0-proof.s)
and [`module.toml`](../../modules/core/adapters/dsp-lp0-proof/module.toml).
`_lp_rx_storage` reserves 4,112 bytes with 16-byte alignment. Existing full run
`out/runs/lp0-proof/5e0b6f2a-84ac-4814-bc6e-57794f493b38` places storage at
`0x20088dc8`, aligned RX at `0x20088dd0`, active payload of 4,100 bytes at
`[0x20088dd0,0x20089dd4)`, and storage end at `0x20089dd8`.
Rounded maintenance covers 65 lines, `[0x20088dc0,0x20089e00)`: 16 bytes before
and 44 after payload, eight before and 40 after storage. Current alignment does
not provide exclusive whole lines. This artifact is not a hardware cache proof.

## Conditional startup configuration

**[D]** Startup `0x1c0e6e` calls `0xb8b93a` at `0x1c0ef6`. M5=0, M6=1,
M7=-1 are installed at `0x1c0e86..8e`; their preservation through preceding
calls has **not** been audited. Under those constants, `0xb8b93a` first calls
`0x1c0272` to clear CFG at `0x3e000`. Range helper `0x1c00da`, selectors
`0x602`/`0x603` and stack argument 1, programs noncacheable ranges:

| Range | Half-open byte interval |
| --- | --- |
| RR2 | `[0x200fa000,0x200fe000)` |
| RR3 | `[0x28240000,0x283a0000)` |

Helper `0x1c008c` validates selectors 2/3 and uses descriptor rows `0x240408`
/ `0x240414`, respectively `{0x3e014,0x3e015,4}` /
`{0x3e016,0x3e017,6}`, selecting CFG2 pairs `0x30` / `0xc0`; other pairs
are retained. Final CFG OR `0x00414141` enables/invalidates I, DM and PM
caches, 16 KiB each. RX lies outside both ranges and is potentially cached.
This conditional trace does not prove the live configuration or residency.
Public semantics: Programming Reference printed pp. 8-9–10, 26-1, 33-2–7 and
35-7–8; helpers were checked against original bytes.

## Stock maintenance helper and limits

**[D]** The inspected interface of helper `0x1c07e3` maps R4 to first address,
R8 to inclusive end, and R12=zero to writeback only or nonzero to DM/PM
writeback plus invalidate. Its effective range/line behavior remains conditional
on correct preceding address normalization; this is not a validated full contract.
Raw instruction `0x1c08a7 = 170078000000` requests DM and PM
writeback/invalidation for the selected maintenance range;
`0x1c08ad = 170050000000` requests writeback only for that range. Neither
invalidates I cache.
The printer's status-stack/no-op interpretation is inaccurate for hardware.

The helper saves/restores CFG2 per line but programs range 0 without restoring
RR0 start/end: range-0 scratch ownership is required. Entry IRPTEN is preserved;
critical sections cover individual lines, not the whole buffer. Prefetch is
invalidated when enabled. There is no invalidate-only operation here: post-DMA
writeback could overwrite DMA data if a dirty CPU line remains.
**[O]** Helper ABI, DAG/SIMD/stack preservation and callback context are unproved;
it is not yet a production callable maintenance API.

**[D]** Existing [`forms_system.py`](../../tools/sharc_dev/sharc_core/forms_system.py)
records maintenance events over coherent backing rather than modeling dirty
cache lines; the transpiler strips the event. Emulator execution cannot prove
physical coherence under this model.

## Correction: address-conversion encoding remains unresolved

**[C]** Raw `0x1c0807 = 04ef08800000` and
`0x1c081b = 04ef88800000` currently decode and execute as Type 7a conditional
MODIFY under NOT SV. The initial audit called these conditional W2B, but that
interpretation is plausible, not unambiguous. Programming Reference printed
pp. 14-46, 14-48 and 14-50–52 cover the relevant encodings; Tables
14-20/14-22 both allow conditional/compute forms; the existing
[`build_table.py`](../../tools/sharc_dev/sharcspec/build_table.py) rationale
selecting 7d only for condition 31 / compute zero is unsupported. Figures
14-19/14-21 overlap; the 7a figure labels `breg`/`toby`. The
[spec notes](../../tools/sharc_dev/sharcspec/README.md) already acknowledge the
contradiction. Both instructions have w/l=11, absent from the documented MODIFY
table; the current core guesses short-word behavior.

**[C]** The independent-source audit also retracts the claim that already-byte
pointers necessarily remain unchanged. Public
[Selache](https://github.com/js216/selache), pinned revision
`2b26d3b75c53063575bc5c820fa0d38879335187`, `selinstr/src/disasm.rs:1076–1105`
tests bit 39 only, ignoring bit 23; no W2B/B2W/ACONV coverage was found.
That coverage gap is not independent proof of MODIFY. The cache-opcode fields
are supported, but the helper's actual coverage depends on the unresolved
address normalization. The RX 65-line arithmetic above is independent of the
helper interpretation.

**[O]** This is a suspected model gap, not a confirmed fix. A universal shift is
unsafe: Table 6-4 conversions depend on the memory map. Hold core edits and
regeneration pending independent encoding evidence. Do not seek approval for a
core correction on the premise that W2B has been proved.

Production pool reservation, cache-safe RX ownership and CF/DSP close/drain
acknowledgement remain open. Completion-dispatch and ownership investigation
continues read-only; no production protocol or sampler readiness is established.
