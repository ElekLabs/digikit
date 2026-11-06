# Experimental host bootscreen assets

This host prototype packs original PNG frames, validates the packed bytes and
previews the decoded result. `.dkbs` is provisional: there is no device loader,
player, transfer or compatibility promise. It does not solve blocked storage I/O.

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

Runtime rendering/storage and publication remain unsolved. A future candidate
is a before-hook at stock scene_setup entry that rewrites the original SP+4
table argument only after publishing a fully validated custom table, then uses
the displaced tail/original setup. This needs ABI, byte and presentation proof;
it is not implemented. Retargeting the indirect task `JSR A2` is not currently
supported by the patcher. Stock skip/completion ownership must remain intact.
