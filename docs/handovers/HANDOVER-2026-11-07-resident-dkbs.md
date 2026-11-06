# Handover 2026-11-07: validated resident DKBS proof

Continued by the [opt-in card provider and Workbench handover](HANDOVER-2026-11-07-card-dkbs.md).

User-authorized RAM-only follow-up on `work/loadable-bootscreens`, based on
`f83518d7`. Eight selected device cases passed on DN2 1.12 / DT2 1.17. No new
hooks, native emulator/snapshot/protocol, runtime/SDK, card I/O, default patch
membership, dependencies, hardware or deadlines. This is a private linked proof,
not a public loadable service/player contract. No push/PR/merge; full gate not run.

## Implementation

The same pure C decoder is cross-built into core/boot-scene-proof and compiled
by the existing Vitest/cc pattern. Whole DKBS validation reuses checked forward
playback for at most 300 ticks, then rewinds/zeros one aligned 1024-byte previous
frame. RAW overwrites, XOR updates in place, holds repeat without decoding.
Duplicated records and valid 1032-byte literal/2048-byte repeat-one payloads are
accepted. No heap, second frame buffer, index/seek or separate validation parser.

Procedural fill_rect drawing and SEEN bits are deleted. Assembly checks the
private optional getter; absence makes no C preparation. Immutable resident
bytes set the table count only after full validation, then publish last.
Each callback checks order and advances one tick before stock blit. The source
is a 28-byte stack view with explicit volatile field stores and an opaque mask;
static ABI assertions cover pointer size and pixels/mask/ownership offsets.
An initial offline link found compiler-generated memcpy for aggregate
initialization; the approved explicit stores fixed it without libc binding.
No destination metadata/ownership writes, constructor or retained source pointer.

Mutually exclusive providers contain only original immutable data. Valid bytes
(2166 B) come from the unchanged Python reference: seeded noise RAW, black RAW,
asymmetric diagnostic XOR held twice, three records/four ticks. Corrupt data is
a compact 40 B black asset with CRC flipped. The committed valid array matches
the reference. Host format and source tool were not changed.

## Actual validation

- Initial host run: 3/4 passed, one 2 s child ETIMEDOUT. Independent identical
  mixed/300-hold/300-sparse workloads completed in 8.0/2.7/4.1 ms with unchanged
  guard; transient cause unproved. One approved unchanged owner retry passed
  4/4 in 2.6 s. Differential bytes, redzones, full denial before outputs and
  malformed header/CRC/bounds/expansion/trailing/file-cap cases are covered.
- Regression types passed in 1.8 s. Final scoped object build: six tasks, 3.4 s;
  both provider producers explicitly built. Actual offline link, generated-module,
  layer and diff checks passed. Python reference's prior 49 tests were retained,
  not rerun; native inputs unchanged, no daemon build or snapshot regeneration.
- First exact DN2 active: 1 passed/9 skipped, 55.41 s total, 51.969 s baseline.
  DN2 held/corrupt/absent group: 3 passed/7 skipped, 153.38 s. DT2 active/held/
  corrupt/absent: 4 passed/6 skipped, 124.14 s. All exit 0, one baseline per each
  of eight scopes. Two missing-receipt cases stay registered, not rerun.
- Both valid cases: four calls, last index 3, three records, status 1, validation
  count 1, errors 0, guard request/entered/returned 1; normal selected loading,
  counter 1 and exact final bitmap hash
  `63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66`.
- CRC-corrupt: status 2/validation 1, no publication/callbacks, guard 1 and normal
  late module load/counter 1. Absent getter: status/validation/publication/calls 0,
  guard 1 and normal late load. Both retain stock black last drawing, not visual
  equivalence of the entire stock animation. Held: KEY 2, guard/validation/calls 0,
  selected module skipped/counter 0; no intro assertion. All eight passed default
  80 ms SETTINGS/NO full-hash roundtrip, IRQ/input, DSP and lifetime checks, with
  zero writewatch/flush hits and no invalid writes. No longer tap waits restored.

Raw logs: `/private/tmp/bootscreen-resident-dn2-valid.log`,
`/private/tmp/bootscreen-resident-dn2-remaining.log`, `/private/tmp/bootscreen-resident-dt2.log`.
Small outcome/retained-directory/baseline index: `/private/tmp/bootscreen-resident-summary.json`.
Each evidence directory keeps only three JSON and intro PNG when present.

## Cost and limits

Actual adapter: text 1116 B, const 0, data 16 B, BSS 2116 B (decoder 1064, mask
1024, instrumentation 28), placed object 3248 B plus existing tail 12 B =3260 B.
The six-byte in-place hook is unchanged. Valid provider separately: text 18 B,
const 2166 B, placed 2184 B. Corrupt provider: text 16 B plus const 40 B.
Metadata: `/private/tmp/bootscreen-resident-placement.json`. No stack-depth,
compressed-flash or performance claim. Final `mise run test:changed` passed
14/14 tasks in 48.0 s (7 cached); final generated-module, layer, owned-format
and diff checks passed.

Drawing capture is before stock presentation, not hardware display/flush proof.
The 64 KiB cap does not prove device staging or cancel SDHC/DMA. Read-only
recovery, cached selection refresh and physical scan/bypass timing remain open.
Card loading, generic provider lifetime and production compatibility are absent.
