# Experimental bootscreen assets

This host prototype packs original PNG frames, validates the packed bytes and
previews the decoded result. `.dkbs` is provisional. An opt-in private card
provider is implemented below; there is no production player or compatibility
promise. It does not solve
blocked storage I/O.

```sh
UV_CACHE_DIR=/private/tmp/digi-boot-query-uv uv run --no-sync python -m tools.bootscreen.asset pack animation.json -o animation.dkbs --preview animation.html
UV_CACHE_DIR=/private/tmp/digi-boot-query-uv uv run --no-sync python -m tools.bootscreen.asset inspect animation.dkbs --preview inspected.html
```

Both print JSON dimensions, nominal 30 Hz, record/tick counts, packed bytes,
raw tick bytes and codec counts. The self-contained HTML embeds PNGs generated
from decoded packed frames, with play/pause and tick scrubbing. No CDN/framework.

## Source and pixel policy

Exact JSON shape (threshold is optional, default 128):

```json
{"version":1,"threshold":128,"frames":[{"file":"frame.png","ticks":3}]}
```

Paths resolve relative to the manifest. Require 1..300 frames, positive integer
holds, total 1..300 ticks; booleans are not integers. Each input must be a
single-frame 128×64 PNG exposed by Pillow in mode 1/L/LA/P/RGB/RGBA. Unsupported
Pillow modes, including 16-bit grayscale, are rejected; export 8-bit PNGs for
predictable conversion (Pillow may expose higher-bit-depth RGB as RGB). Inputs
convert to RGBA, composite over black
using Pillow, then integer luma `(77*R + 150*G + 29*B + 128) >> 8`; pixels are on
strictly above threshold (integer 0..255). No resizing, dithering, SVG or APNG.
Consecutive identical native frames merge by summing holds.

Native byte `8*x + y//8`, bit `7-y%8` is column-major, MSB-first top to bottom.
The reused `tools.control.digi_client.oled_png` uses a different layout:
byte `8*x + 7-y//8`, bit `y%8`. Preview explicitly reverses byte and bit order
per column; asymmetric-corner tests cover this conversion.

## Provisional binary v1

All integers are big-endian; no trailing bytes.

| Part | Encoding |
| --- | --- |
| Header, 16 bytes | `>4sHHII`: `DKBS`, version 1, record count, total ticks, body CRC32 |
| Record, 8 bytes | `>HHB3x`: hold ticks, payload bytes, codec, three zero reserved bytes |
| Payload | The declared number of bytes |

CRC32 is standard `zlib.crc32` over the entire body. Record count and total ticks
are 1..300; each hold is positive, and their sum must match the header. Payload
length is nonzero and must fit the remaining file; RAW is exactly 1024 bytes,
while XOR-RUN must expand to exactly 1024 (even if its encoded stream is larger).
The encoder merges identical consecutive frames; the decoder
also accepts valid repeated-frame records without requiring that optimization.

- Codec 0 RAW: exactly 1024 native bytes.
- Codec 1 XOR-RUN: XOR the current native frame with the previous decoded frame
  (initially zero). Control 0..127 gives a literal of `ctrl+1` following bytes;
  control 128..255 repeats the next byte `(ctrl&127)+1` times. Expansion must be
  exactly 1024 bytes, and bounds are checked before each expansion. There is no
  dictionary, index or extra mode. The encoder uses XOR-RUN only when strictly
  shorter than RAW, otherwise RAW.

The host caps the whole file at **65,536 bytes**. This is a chosen experimental
policy, not a bound proved from DKM staging or device memory. Dense incompressible
animation may not fit and fails with size/max instead of silently dropping data.
Decoder checks cap/header/counts/CRC, reserved bytes, codec/length, expansion,
exact consumption and hold sum. CLI reads at most cap+1 bytes when inspecting.

## Host evidence and remaining integration

Named check: `uv run --no-sync python -m pytest tests/bootscreen/test_asset.py`.
Scoped Ruff and mypy pass. Generated preview JavaScript passes Node syntax
checking; interactive preview was not checked because no browser is available
on this host. Existing Pillow is reused through the declared
matplotlib development dependency; matplotlib itself is never imported. No
new dependency or environment change.

Original synthetic 180-tick measurements under ignored
`out/runs/bootscreen-assets-host/`: moving dot `(x=i%128,y=(3*i)%64)` is **5,715 B**,
180 XOR-RUN records; one constant dot merged from 180 frames is **42 B**, one
XOR-RUN record. Each represents 184,320 raw tick bytes. These are disk sizes for
these synthetic inputs, not general compression or performance claims. HTML
previews and source PNGs remain ignored artifacts, not repository assets.

The private resident proof implements the stock scene_setup entry hook: it rewrites original SP+4 only after
publishing a validated resident table, then uses the displaced tail/original
setup. Stock skip/completion ownership stays intact. This is not a public player
or card-lifecycle contract. Retargeting the indirect task `JSR A2` remains
unsupported; no task replacement is used.

## Private resident interoperability proof

The opt-in core/boot-scene-proof now cross-builds the same pure C decoder tested
against this Python reference, without changing this format or host tool. Whole
asset validation uses the playback decoder, at most 300 ticks, then rewinds one
aligned 1024-byte previous-frame buffer. Noncanonical valid repeated records and
oversized encoded XOR streams remain valid; output is bounded to 1024 bytes.
The initial interoperability proof used immutable compiled RAM providers. The
separate card provider below extends that private boundary without a public SDK.
The scene count is published after validation. Existing stock blit receives a
plain synchronous source view plus opaque mask; stock setup/ownership/completion
stay intact. Eight selected current-device emulator cases pass (valid, CRC-corrupt,
absent getter and held REC); all use the existing 80 ms UI taps and full roundtrip
checks. This demonstrates a narrow integration, not production loader,
storage cancellation or physical presentation compatibility. See the
[resident proof handover](../handovers/HANDOVER-2026-11-07-resident-dkbs.md).

## Opt-in card provider contract

`core/boot-card-proof` is a separate firmware adapter requiring the private
`core/boot-scene-proof`; neither is in a default patch set. It supports DT2 1.17
and DN2 1.12 and conflicts with the two linked RAM fixture providers. This is
independent of any future loadable-service contract: no early `.dkm` loading,
public SDK addition, SELECT change or settings field is introduced.

The only active path is **`bootscreens/active.dkbs`** in the generic digikit
store on either device. Existing atomic PUT replacement activates a new file
for the next boot; deleting that path restores stock fallback on the next boot.
The current session retains its own immutable copy, so upload or deletion never
changes an animation already prepared. There is no selection-list membership.
Firmware must include the opt-in provider for this path to have any effect.

The existing early guard must finish runtime initialization, counter arming and
exception protection before it calls the getter. Held REC, a safe boot or missing
panel receipt skips the getter. The provider stats the fixed path, rejects sizes
outside 16..65,536 bytes, allocates exactly the reported size, then calls existing
`dkrt_api.store_read`. That call checks staging fit and whole-file SHA before
copying into the owned block. A positive result no greater than capacity supplies
the actual size; all other read results free the block. Missing files, invalid
sizes and failed allocation return no asset. There is at most one allocation
attempt and no retries. No storage directory or selection call is needed.

The scene adapter validates the entire DKBS before publication. Rejected bytes
invoke the optional private discard callback, then the decoder pointer and valid
state are cleared. Linked immutable fixture providers need no discard callback.
Successful card bytes remain allocated for the entire boot/session until reset;
the intro callback never frees them because main-task arena allocation may run
concurrently. Failed loading or validation leaves stock animation setup intact
and permits normal late module loading.

The runtime arena rounds the requested size to 16 bytes and adds its 16-byte
header: a 2,166-byte fixture reserves 2,192 bytes; the 65,536-byte cap reserves
65,552 bytes. This is additional to the retained decoder (1,064 bytes), opaque
mask (1,024 bytes), scene table and instrumentation. The provider has no large
BSS or staging alias. Its current object is 246 bytes text, 24 bytes constant
path and 32 bytes BSS, placed in 304 bytes including alignment. The scene object
is 3,284 bytes plus the unchanged 12-byte hook tail; the hook write stays 6 bytes.
These are offline build costs, not stack-depth or compressed-flash measurements.

Host coverage compiles the production provider with the real generated C API
headers and fake stat/allocator/read functions. It checks exact allocation/read,
retention, missing/short/oversized inputs, allocation/read failures, actual read
size, idempotent discard and no retry. The existing four C/Python codec tests
remain the format oracle. The generic store's same-path replacement unit checks
commit cuts with already written old/new payloads in separate extents: the
visible entry, extent, size, digest and bytes remain consistently old or new.
It does not prove interrupted upload DATA, hardware flush or power-loss timing.

Device cases are registered for valid, missing, corrupt, oversized, held REC,
empty selection and seeded counter bypass on each device. Ten selected cases
passed: all seven DN2 plus DT2 valid/corrupt/held, including retained bytes after
late loading, invalid-asset freeing and the existing UI/DSP/input/lifetime proof.
A further DN2 USB PUT/VERIFY/delete case passed while the current owned asset
remained unchanged. Eleven unique device cases passed; five DT2 scopes remain
registered, not run. Detailed results are in finding 115
and the card handover. Existing store I/O can block; this provider
does not establish a deadline, storage cancellation, read-only recovery or
physical display/scan timing guarantee.

## Workbench transfer

With firmware that includes the opt-in provider, open **Transfer → Bootscreens**,
choose **Add .dkbs**, then **Upload**. Workbench PUTs the file to the fixed generic
store path on either device and performs a fresh VERIFY before reporting
completion. The active file is listed there; **Remove active** requires a second
click to confirm deletion. Upload and removal take effect on the next normal
restart. Workbench does not restart the device or change module selection.

Workbench rejects files outside 16..65,536 bytes before reading their contents
and checks only the DKBS magic, version and record/tick ranges. Use the pack/inspect
tool above for complete validation before transfer; the device validates the
whole asset again at boot. Workbench cannot detect whether the installed
firmware includes this experimental provider, so the transfer panel states that
requirement. The existing Samples destination and public protocol/SDK remain
unchanged.

## Returned-I/O error proof, phase 1

The private opt-in `core/storage-error-proof` entry shim and pure-data
`test/boot-io-*` configurations model returned block-driver errors, separately
from the DT2 IRQSTAT observer. Before `BOOT_GUARD_ATTEMPTS` is nonzero they pass
all I/O without observations; afterward only requests starting within the
runtime store match. No buffer reaches the C decider. The pass path restores
volatile registers and executes patchkit's displaced prologue; rejection returns
-1 directly to the original caller. The base intentionally requires one mode
configuration at link time and configurations conflict. Neither adapter nor
configuration belongs to a default image.

Phase 1 registers a pass control and one-copy recovery on DT2 1.17 and DN2 1.12.
The seed has valid empty selection A sequence 1 and newer c-demo selection B
sequence 10. B's first early 512-byte read returns an error once. The required
behavior is no early asset/callback, followed by a fresh late read preserving
the newer selected path and loading c-demo with counter 1. The suspected bug is
accepting older A after B's returned error, then overwriting B when arming A.
The test saves boot, provider/guard/scene, selection copies, fault counters and
intro evidence before assertions. Only after parent review will the pass DN2
control run, then one expected-red DN2 recovery case without retry. No runtime
fix is part of this phase.

Other configurations have host-only semantics in this phase: each selection
copy once, first 2,560-byte payload read once, persistent store write errors,
and first header A read once. These model returned errors, including a
read-only returned-error model. Actual DMA failure, hardware write protection,
bounded blocked I/O and cancellation remain open.

The before-fix DN2 one-copy run reproduced loss of the newer B selection.
The bounded runtime correction preserves per-copy I/O failure separately from
invalid decoding and denies early preparation after any selection read error,
resetting preparation so the late normal path rereads both copies. Existing
late persistent-read-error policy is unchanged and remains open. Host coverage
checks both error directions, both errors, invalid CRC, ordering/ties and a
scripted fresh late read with counter arming from the newer selected record.
