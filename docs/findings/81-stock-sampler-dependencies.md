# Stock sampler dependencies and the next SDK proof

2026-10-29. Targets: DT2 OS 1.17 and DN2 OS 1.12. **[D]** records current-database/source observations;
**[O]** records unproved semantics or proposed work. **[V]** is restricted to the independently audited
original-image byte facts below; it does not cover the broader dependency/lifetime interpretation.
This updates the next-step framing in [finding 80](80-sampler-capacity-and-replacement.md), preserving its
capacity measurements and the completed immutable-bank proof in [finding 79](79-sample-bank-proof.md).

## Direction

The user wants stock DT2 OneShot and other machines to reveal the dependencies the SDK needs, rather than
wholesale copying of a stock machine. Confirmed product intent remains approximately ten-second one-shots,
stopping old notes on selection. Mono is the first proposed implementation scope; stereo is not approved here.

**[O]** The coordinator proposes an independently written shared **packed-PCM16 bounded reader, cursor and
region semantics**. A device adapter supplies a ready immutable view; runtime owns slot selection/loading and
lifetime. Stock downstream envelope/filter processing remains in place. DT2's direct stock view cannot be handed
to the existing float32 normal-word reader in `sample.inc`: representation and pointer contracts differ.
This is a design proposal, not an implemented ABI or a claim of stock-quality equivalence.

## DT2 current-image dependency map

| Dependency | Current DT2 1.17 observation [D] | Open boundary [O] |
| --- | --- | --- |
| Project acquisition / allocation | General loader `0x4015481c` accepts slots ≤`0x3ff` except multiples of `0x80`; rounds allocation as `(size + 0x200f) & ~0x1fff`, with offset limit `0x19000000` (400 MiB), and deduplicates references. | Allocation ownership, safe adapter entry and all failure paths require controlled proof. |
| Slot → block / delivery | Slot map `0x405b5368 + slot*4` reaches block vector `0x405a0f64..0x405a0f68`, stride `0x34`. Pages are sent by `0x40153a10`; headers by `0x40153e6c` / `0x40153e04`. | Page/header completion and cache/read visibility are not a reader-lifetime barrier by themselves. |
| Release / project reset | `0x401543a2` decrements references, coalesces on zero and clears the header through `0x40153ec6`. Reset `0x40154634` clears mappings and allocator; `ProjectSettings::ReloadAllSamples` at `0x4004e8be` queues worker work then resets/reloads. | Active/queued voice behavior across these operations needs capture before an adapter borrows a view. |
| Preview correction | `0x40154112` is preview: fixed slot `0x400`, offsets `0x19000400` / `0x19100400`. | **[C]** This is not the general project allocator; do not build the SDK contract from that earlier lead. |
| Kit selection | `0x4004e598` uses record `+0x80` for sample slot and `+0xd6` for machine type. | Precise frame `0xe0 + 0x60*t` remains a historical derivation, not a newly established current-image field contract. |
| Slot → voice setup | `0x1c3289` → slot lookup `0x1c3f78`, continuation `0x1c3fa8` → `0x1c7442` → `0x1c4e70`. Slot table candidate `0x257810`, rebased address `0x8422b9c8`; page copy `0x1c403c`. | Capture a real selected slot and resulting voice pointer. At `0x1c4e91`, `DM(I4,M4)=R8` with `M4=0x65` is post-modify: the store is word 0, **not word +0x65**. |
| Render branch | Dispatcher `0x1c642a` routes non-Stretch through `0x1c4ecf` → `0x1c4f81`; selector 2 Stretch goes `0x1c5576` → `0x1c5615`. | Specific OneShot naming and parameter mapping remain historical DT2 1.16 evidence until current controlled selection. |
| Reader / boundaries | Six shortword taps at `0x1c50b4`, `0x1c50c2`, `0x1c50cd`, `0x1c50d2`, `0x1c50d7`, `0x1c50db`; current database has a 64-iteration loop and coefficients at `0x25d940`. Header path `0x1c7e76` → `0x1c400f`. | Exact guarded span is unresolved. Detailed Q31 boundary, reverse and stop semantics in finding 06 around line 3518 are historical; do not promote them to current-image proof. |

### Independently audited instruction bytes

**[V]** A separate auditor decoded freshly extracted original DT2 1.17 image bytes. Stock SYX SHA-256:
`26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5`; section 7 is 321,016 bytes,
SHA-256 `0f514a12a2255f5c081e292c47f1f29462003177658da4bbae0a22fd737fffa2`, matching the extraction.
The verified scope is:

- `0x1c3fc6` raw `16448422b9c8` and `0x1c3fcb` raw `164d8422b9c8` modify by adding `0x8422b9c8`.
- `0x1c4e8f` raw `0fa40065` loads M4=0x65. `0x1c4e91` raw `893e84001f20` is the unconditional Type 6a
  post-modify store through I4/M4 of R8: store at the original pointer, then modify it.
- The six taps are Type 3b signed-shortword loads (`l=1,x=1,w=0`, SWSE): `0x1c50b4=497e40bd`,
  `0x1c50c2=577e413d`, `0x1c50cd=497e42bd`, `0x1c50d2=497e403d`, `0x1c50d7=59be40bd`,
  `0x1c50db=597e413d`. Their `u` bits are mixed: 1 at `0x1c50c2`, `0x1c50d7`, `0x1c50db`, and 0 at
  the other three. Do not infer uniform addressing mode across the taps.

The public [SHARC+ PRM revision 1.5](https://www.analog.com/media/en/dsp-documentation/processor-manuals/sc58x-2158x-prm.pdf)
Type 3b BHSE tables (printed 14-19/20) and Type 6a tables (14-42/44) support these decodes. This audit does
**not** verify guarded extent, full control flow, allocator/lifetime semantics, capacity or OneShot mode naming.
Read-only reproduction uses `container.sections` → `extract.classify` → `elz.depack_section` →
`LoadedMemory.from_stream` → `decode_confident_loaded` with `.venv/bin/python -B`; no generated core is required.

The loader's 400 MiB offset bound and the manual's documented project capacity are distinct observations;
neither proves an SDK allocation or a safe borrowed-view lifetime. Release/reset evidence makes lifecycle a
concrete dependency rather than an assumption that an address stays valid.

## DN2 comparison: dependencies, not playback feasibility from scratch

**[D]** Read-only `Image.match` and NetworkX comparisons found no relocation-hash matches in DN2 for the listed
DT2 slot/setup/page/renderer functions. This is **not evidence that functional equivalents are absent**.
The exact-hash common pair DT2 `0x1cdd8b` / DN2 `0xb8251b` is a downstream per-voice hook, not a sample reader;
DN2 engine `0x1c8ef1` reaches it.

The existing DN2 adapter already plays samples through its voice seam, private source acquisition and float32 L2
bank. Preserve that working path. **[O]** Larger owned backing and delivery remain missing contracts, distinct
from playback. Investigate those gaps against the concrete DT2 dependency list, rather than restarting from a
claim that DN2 sampling is infeasible. External-looking addresses, a CF cave or module arena alone establish
neither owned DSP memory nor a delivery path; do not assume DT2's stock pool or LP0 exists on DN2.

## Executed follow-up

[Finding 82](82-stock-sampler-lifetime.md) records the scoped lifecycle discovery, image identities, retained
captures and stock-only reproduction status. OneShot selection, stored slot binding, verified ten-second input
and a passing active replacement reproduction on the original unpatched DT2 1.17 image are now recorded.
Start/end/reverse, removal and project reload
remain open; the full lifecycle is not complete. The result requires explicit retirement before overwrite,
including external replacement/reset paths, rather than treating a metadata acknowledgement as a barrier.

## Bounded next proof, before implementation

- [x] Select stock DT2 OneShot on OS 1.17 and capture selected slot → stored voice binding with actual PCM;
  active-note and image-specific replacement evidence is separated in finding 82.
- [ ] Capture start/end/reverse behavior and sample-change/project-reload transitions, including old-note stop behavior.
- [ ] Use those observations to specify adapter readiness, retention, invalidation and reader-quiescence appropriate
  to the actual stock contract; transport ACK alone is not that barrier.
- [ ] Specify a shared packed-PCM16 reader/cursor/region contract independently, then prove it numerically before adoption.
- [ ] Map each DT2 dependency onto the already working DN2 seam; bound investigation of missing larger backing
  ownership and delivery. Decide the next probe only from those concrete gaps.

**[O]** After the proof design is agreed, reuse the existing TypeScript regression session APIs in
`sample-playback.test.ts` under `tests/regression/runtime/` for scheduled input, DSP peeks (including unknown-word
counts) and PCM capture. Keep a selected-slot change distinct from REPLACE/removal of a referenced slot and
project reset/reload: selection may retain the old allocation, whereas the latter operations can reuse its extent.
Use distinct deterministic PCM fixtures to expose stale old reads versus new data. Capture the CF slot map/block
reference count, DSP metadata and active voice pointer before and after each transition. Hearing the new sample
or observing READY alone does not prove reader quiescence. This note adds no scenario or test implementation.

No implementation, stereo decision, throughput claim, new pool ownership or hardware validation is established
by this note. Current immutable-bank acceptance remains unchanged.

## Reproduction

Use the read-only firmware query CLI, with existing current-image inputs. For a function dossier:

```sh
uv run python -m tools.coldfire.fw dossier 0x4015481c -i dt2-1.17 --decomp 80
```

Repeat the dossier for the CF entries in the table. The existing `batch` command accepts one such command per
line without the Python/module prefix; batch the entries to share query setup. DSP listings use the same image:

```sh
uv run python -m tools.coldfire.fw disasm 0x1c4e91 -i dt2-1.17 --cpu sharc --max 16
```

Do not implicitly rebuild shared databases. Image-match results establish only the stated hash comparison;
retain the distinction between database observations, historical semantics and a future controlled runtime proof.


## Project invalidation follow-up

**[D]** Current DT2 1.17 database dossiers retain the slot/path loader at
`0x4015481c`, slot map `0x405b5368`, and loaded-block vector globals
`0x405a0f64/68/6c`. Release `0x401543a2` removes a slot mapping and only
releases/coalesces the associated block when its record reference count at
`+0x30` is zero. `ReloadAllSamples` at `0x4004e8be` can queue the reload
path through `0x40154634`, which clears the 1024 slot mappings via
`0x40153ec6`, resets the vector and emits the stock sample event.
These are concrete invalidation paths for any proposed borrowed view.

**[C]** Candidate `0x401363d0` is a byte-fill helper (destination, byte value,
length), not a sample allocator. Its repeated-byte word fill and tail stores
do not supply an independently owned DSP extent.

**[O]** The stock loader remains useful evidence, but an adapter still needs
an ownership contract and a reset/replacement fence before reusing its pool.
The dossier observations do not establish either contract.


## Explicit-tail reservation candidate

**[D]** A bounded DT2 1.17 loader follow-up distinguishes the initial rounded
block-size check at `0x40154aea` from a shared pool boundary. The loader also
reuses gaps and grows/merges records. Helper `0x40154682`, called by
`0x40154770` and the loader at `0x40154c10`, derives the next offset from the
last vector record (or `0x400` for an empty vector) and compares the proposed
end against `0x19000401`. Its decompiled return encoding is not a complete ABI
proof. The vector's storage-end pointer `0x405a0f6c` concerns metadata capacity,
not an independently owned sample tail.

**[O]** Lowering a single 400-MiB literal does not establish a carveout.
A reservation must constrain initial allocation, append/growth, gap reuse and
reset/reinitialization consistently. The current pass identifies those enforcement
questions; it has not implemented or validated a smaller stock pool. Borrowing a
stock allocation remains another route only with a fence before reclamation.
