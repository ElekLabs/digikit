# Private DT2 storage-fault observer

Opt-in `core/storage-fault-proof` targets only DT2 OS 1.17. It is not in a default
module set, a sample-load gate, recovery mechanism or public module ABI.

Three `replace` hooks target the audited six-byte IRQSTAT→D0 loads in storage ISR
`0x4012d5ba`: offsets `0x08`, `0x5c`, `0x88`. Each reads MMIO `0xfc0cc030` exactly
once, ORs only bits `0x00310000` (command timeout, data timeout, CRC) into the
four-byte `storage_fault_bits`, and jumps to its named site plus six. It must never
jump to a displaced-code tail, which would perform the read twice. D0 and the
load's N/Z, cleared V/C and unchanged X are retained, with all other registers,
full SR control, A7 and the existing exception frame preserved.

Each hook uses 13 instructions and eight bytes below A7: saved D1 at SP+0 (four
bytes) and saved post-load SR at SP+4 (two bytes), with two unused bytes. LEA and
MOVEM precede the SR snapshot without changing CCR. The one memory-destination
OR.L merges the current sticky word in a single CPU instruction; an observer
running between instructions cannot lose another observer's bits. This does not
cover concurrent non-CPU writers or prove whole-ISR/controller behavior. No IRQ
mask change, heap, logging, firmware call, counter or clear operation is added.

The linker-owned BSS word is initially zeroed by the existing expansion segment
loader, before the traced MAIN storage initialization. It is separate from
runtime-loaded DKM BSS and is never reset by selection/load/UI/runtime hooks.
Bounded native traces observe loader zeroing before FS initialization and host-seeded
retention, but the full fault-history acceptance remains failed/open: it records
zero observer writes. The observer covers only these three snapshots and audited error bits;
quiet output is not health, cache-history, full fault coverage or DMA-drain proof.

The compiled-source owner test is `packages/patchkit/tests/storage_fault_observer.rs`:
`cargo test -p patchkit --test storage_fault_observer` with an installed ColdFire
toolchain, under the shared benchlock and an isolated host target. It assembles and
links this actual source, then uses Cpu/custom Bus checks for MMIO-read count,
context/exception frame, exact write footprint, sticky union and observer nesting
at every instruction boundary. No firmware bytes are stored in these tests.

A nonzero readiness value must be recorded and classified, not assumed to be an
unsafe sample-source failure: startup might contain handled probe errors. Native
acceptance must observe loader zeroing and sticky accumulation rather than assume
zero at ready. This raw observer does not classify all captured bits as unsafe
runtime source faults or implement the accepted sample-load policy.

Independent static review of the patched image checks the actual segment chain,
not only the plan: adapter payload 164 bytes at `0x47e8b000`, BSS four bytes at
`0x47e8b0a4`; DKPR payload 92 bytes at `0x47e8b0a8`, no BSS. Reset `0x4000053e`
reaches `pk_loader` at `0x403117c4`; loader relocation starts at stock MAIN end,
and its final helper calls reach `0x4000045c/0x400004b2`. Patched MAIN SHA-256 is
`6ba724b86296c238e068806c355af8f180f9c43eb5909f1b5ea331198a84c559`.
These are static artifact checks. Default native storage completion uses Oracle
semaphore posting rather than establishing stock ISR execution; the full native
fault-history check remains **failed/open** with zero observer writes.

The opt-in [controlled ISR fixture](../../../../packages/patchkit/tests/storage_fault_isr.rs)
loads the exact stock DT2 1.17 `.syx` at runtime (SHA-256
`26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5`),
verifies each original hook-site hash and installs assembled hooks only in the
in-memory copy. It executes the actual stock ISR, both signal helpers on their
no-waiter path and the ISR's RTE. Synthetic vector/level setup enters through
`Cpu::take_interrupt`; a sparse bounded Bus supplies a controlled W1C IRQSTAT
test double. No firmware bytes are embedded.

The named test passed 36 sequence/SR combinations: eleven single-interrupt cases
and one five-interrupt sequence, each with three SR values, for 48 interrupt
invocations per image (stock and patched). It checks each audited error bit,
completion-only and simultaneous error/completion cases, and sequential sticky
union. Stock/patched register/SR/PC/A7 results, exact MMIO reads/W1C writes,
nonobserver write traces and status/signal globals match; the observer latch and
its checked temporary stack writes are excluded from the write comparison.
Each replay has a 2,048-instruction ceiling.

From the repository root, with the shared benchlock configured for the worktree:

```sh
UV_NO_SYNC=1 UV_CACHE_DIR="$PWD/out/cache/uv" \
DIGI_STORAGE_FAULT_ISR_SYX="$PWD/firmware/Digitakt_II_OS1.17.syx" \
uv run python -m tools.devtools.benchlock run --shared --timeout 120 \
  --label storage-fault-isr -- rustup run 1.98.1 cargo test --offline --locked \
  -p patchkit --test storage_fault_isr \
  --target-dir out/native/target-wt/digikit-dsp-sampler/nogen/storage-fault-proof
```

Without `DIGI_STORAGE_FAULT_ISR_SYX`, the fixture skips with a reason; when set,
a missing, unreadable or wrong-hash input fails. This is **[D]** component evidence,
not controller delivery, DMA, waiter/scheduler behavior, hardware or native-boot
IRQ validation. Full relevant fault history and the production sample-load gate
remain open; see the [plan](../../../../docs/plans/dsp-sdk-and-sampler.md#next-proof-private-early-dt2-storage-fault-observer).
