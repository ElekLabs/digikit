# Bounded DDR access diagnostic

2026-10-30. **[D]** implementation and focused host tests; **[O]** compiled-backend,
Wasm and stock coexistence validation. See the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Contract

`Mem::set_ddr_monitor` and `Engine::set_ddr_monitor` accept an optional half-open
byte interval within `[0x80000000,0xa0000000)`. The diagnostic records the first
known-address, supported-width overlapping attempt, with address, width and kind.
It leaves ordinary access results unchanged and does not block accesses. It is
not a production reservation, capacity guarantee, hardware watchpoint or security
boundary. No CLI, daemon or public module ABI is exposed in this first step.

CPU read/write attempts are distinguished from storage events. CPU hooks run
before uninitialized-value synthesis, model rejection and journal old-value reads,
including known-address writes whose value is symbolic. Unknown addresses and
unsupported widths are outside this contract. Storage events include host/DMA
memory operations, import/export and journal/rollback traffic; they are not guest
attribution. Duplicate helper calls cannot replace the first latched event. There
is no exact access-count or exact instruction-PC claim.

Setting, replacing or removing a valid range clears the latch and invalidates
resolved windows. Invalid ranges leave configuration and the latch untouched.
Memory reset preserves the configured range but clears the latch; later import
writes may produce storage events. Arm after setup/import and inspect the latch
before exporting memory when the intended experiment concerns guest execution.

## Fast-path and idle coverage mechanism

Watched DDR pages retain their contents but withhold their exported fast pointer.
The policy is reapplied when pages are allocated or refreshed. Cached static and
dynamic windows are invalidated using `layout_gen`. The shared flat-access IR
checks null pointers before native or Wasm raw loads/stores; a declined window
is not itself recorded as an access. Normal execution can then fall back to the
observed memory/semantic boundary. Monitor transitions invalidate idle-loop
snapshots/probes, and idle skipping is disabled while the monitor is active.

Implementation owners are `packages/emulator/sharc-rt/src/mem.rs`, its handwritten
`rt/bnd.rs` semantic boundary, and `packages/emulator/sharc/src/lib.rs`. Tests cover
memory policy, semantic attempts, idle lifecycle, window invalidation and common-IR
fallback. No Python instruction semantics, generated core, firmware bytes or
module/runtime objects changed.

## Validation and open boundaries

Eight focused monitor tests pass; all 21 runtime-crate unit tests pass (counts
overlap). Independent source review found no issues. The common-IR test executes
the reference interpreter and explicitly continues through the semantic boundary;
it does not yet prove automatic full-engine dispatch through the compiled backend.
Same-value stores, read-only accesses, overlap boundaries, invalid ranges, first
latch retention, reset/removal, synthesized reads and rejected symbolic-value
writes are covered within those tests.

**[O]** Reusing the existing native flat-access test harness reproduced the
pre-existing Cranelift block-builder panic documented in the October 27 handover.
It aborts through the backend C ABI. The diagnostic leaves that compiler unchanged;
actual compiled-backend and Wasm execution remain required before claiming their
coverage. Root inspection identifies `Emit::fail_block` switching away from the
current block before its guard branch is emitted as a candidate cause, pending
reproduction and a separately scoped fix. A reference-interpreter pass is not a
substitute for that backend proof.

Stock CPU/DMA coexistence runs, complete integration, and physical memory/cache
behavior remain open. Even after backend validation, a quiet bounded run cannot
establish universal firmware ownership of a candidate sample pool.


Focused validation used `CARGO_TARGET_DIR=out/native/target/ddr-monitor`,
`CARGO_BUILD_JOBS=2`, the shared repository benchlock and no generated-core
variables. Commands under `tools.devtools.benchlock run --shared`:

```sh
cargo test -p sharc-rt -p sharc-native --lib ddr_monitor
cargo test -p sharc-rt --lib
```

Both returned zero. Output was retained in tool sessions only; no standalone run
log was saved. Reusing this target avoids an unnecessary fresh build for follow-up.


Formatting: owner-wide `cargo fmt --check` reports existing unformatted generated
and handwritten sources. A scoped check of the six changed files identified three
new formatting hunks, which were corrected; unrelated formatting was retained.
This is not an owner-wide formatting pass.
