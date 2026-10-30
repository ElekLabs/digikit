# Stereo machine output and channel history

2026-10-30. DT2 OS 1.17 / DN2 OS 1.12. **[D]** source inspection and bounded
original-byte audits; **[C]** corrected interpretation; **[O]** integration open.
See the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Current SDK boundary

**[D]** Machine contract 1.1 has one 32-float output pointer in `dk_voice_in`.
The DT2 adapter renders the even voice and explicitly copies its output into
the paired odd voice at `_dv_odd` in
`modules/core/adapters/dsp-voice/src/dsp/dsp-voice.s`. DN2's adapter passes one
32-float slot buffer to the module before the stock mono processing chain.
The separate FX contract already has two 32-sample planes; that does not make
machine output stereo. A second field in the SDK input alone is insufficient.

## DN2 channel split: correction

**[C]** The historical description of `0x1c9aea` as 32-to-64 temporal upsampling
is contradicted by current original bytes. This includes the legacy description
in the DN2 adapter's source comment. The code creates **two 32-sample planes**
from one 32-sample mono block, multiplying each input by separate gain streams.
It does not double the temporal sample count.

**[D]** DN2 1.12 SYX SHA-256:
`7e5b8a733d4ab1fdf7e5d22757f7d8eddaa3cbf71d0858874a791844c02ceb35`.
Section 7 is 836,956 bytes, SHA-256
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
The auditor freshly extracted these bytes and checked the listed instructions.

Initialization at `0x1c14e1..0x1c1575` builds pointer table `0x258ee8`:
channel A for slot v is `0x257ee8 + 0x100*v`; channel B is A+`0x80`.
The table is installed through `0x1c2848/4b` and read by the engine at
`0x1c9add`. At `0x1c9af5` one input is loaded; `0x1c9af6/aff` multiply that
same sample by two coefficients and `0x1c9af9/0x1c9b0c` store the pair.
The loop repeats for the next input at `0x1c9b1b/21/2d`.

The gain sources include complementary tables `0x8052d790/0x8052d990`, with
endpoints `(1,0)` and `(0,1)` and midpoint `(0.70710683,0.70710683)`.
This supports pan-channel interpretation; physical connector ordering was not
separately traced. Downstream `0x1c1738` and `0x1c1f1e` read paired planes
32 words apart and process 32 sample pairs. Caller order is engine at
`0x1c3044`, then those consumers at `0x1c3054/5d`.

Initialization writes 48000.0 to `0x257e5c` at `0x1c1487` and block count 32
to `0x257e6c`. This establishes configuration and temporal-count preservation,
not a physical clock measurement. There is no 96 kHz conversion in this loop.

## Mono processing before that split

**[D]** Hook `0xb8251b`, called at `0x1c9459`, receives one slot buffer as
both R8 and R12. Buffers start at `0x804acf90`, stride `0x80` bytes. Scalar
gain and later stages process that buffer in place before the split. Two finite
stage audits establish independent audio histories without internal control
advancement:

| Stage / call | State per slot v | Coefficients read | Audio history written |
| --- | --- | --- | --- |
| `0xb80c39 → 0xb809f2`, call `0x1c94fd` | `0x24f320 + 0x70*v` | Words 21,22,23 | Words 24,25 |
| `0xb81368`, conditional call `0x1c959f` | `0x2533e0 + 0x20*v` | Words 2,3,4,5 | Words 6,7 |

Both process the same input/output buffer, with a normally configured count of
32 and sixteen slots. In these finite bodies, other accesses are sample I/O and
stack traffic; there are no global/RNG writes or coefficient/ramp updates.

The first stage is `y = a*x + b*x_previous - c*y_previous`, then updates the
two histories. The second stage, with coefficients `a,d,b,c` in words 2..5, is:

```text
Unew = a*U - d*x
Ynew = c*Unew - c*U + b*Y
```

The second stage loads coefficients once per block. Its separate float products
and subtraction order matter. Under fixed coefficients, its real-arithmetic
transfer function can be expressed as the SDK biquad with
`b0=-c*d, b1=c*d, b2=0, a1=-(a+b), a2=a*b`. That is **not** established as a
bit-identical replacement: a float32 example already differs by one ULP, and
state conversion depends on coefficients. Do not collapse the recurrence or
reuse one channel's history for both channels.

**[O]** These stages can share captured coefficients while retaining independent
left/right histories. This does not establish the same property for the entire
chain or exclude external coefficient changes. Remaining work is an ordered
stage/state inventory, shared control evolution once per block, independent
channel processing, and the existing pan/send path. A late stereo insertion
alone would bypass earlier mono processing and is not the selected solution.

No module ABI change, firmware patch, emulator run or hardware test is claimed
by this finding.
