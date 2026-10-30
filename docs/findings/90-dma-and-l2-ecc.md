# 90. Stock DMA targets and L2 ECC against the module regions (the freqshift freeze)

The question comes from the freqshift freeze on Em's DT2 (finding 77, docs/plans/sdk-api.md "Freqshift freeze on the DT2"). Does
something that the emulator does not model write or check the memory where run-time modules go? Two candidates: a stock DMA channel that
writes into `dsp.l2`, and L2's ECC. Marks as in AGENTS.md: **[D]** measured here (emulator, static), **[O]** open. Nothing is **[V]**.

Short answer: neither one explains the freeze. No DMA channel that stock sets up writes outside L1 block 1, and the freqshift path makes no
write that L2's ECC refuses **[D]**. The cause stays open **[O]**.

## 1. Method

1. Run a stock ready session with `--dump-dsp`. The engine keeps every MMR write in `State.mmrs`, and the dump holds them with every
   memory byte written since the image:
   - DN2: `uv run python -m tools.emulator.coupled scenario trig1-taps --dump-dsp --no-cache`. The default DN2 image is 1.12. The run is 10.5 audio s.
   - DT2: `uv run python -m tools.emulator.ready dt2`, then `coupled scenario dt2-sample-trig --device dt2 --dump-dsp --no-cache`.
     The image is 1.17 (sha256 `26c22f6652625ac2`). The run is 3.0 audio s.
2. For each DMA channel in HRM Rev 1.1 Table A-18, read `DSCPTR_NXT`, `ADDRSTART`, `CFG`, `XCNT`, `XMOD`, `YCNT`, `YMOD` from the MMRs.
   `CFG` (p. 27-50): EN bit 0, WNR bit 1 (the channel writes memory), MSIZE 10:8, FLOW 14:12 (4 = descriptor list), NDSIZE 18:16, TWOD 26.
3. For a descriptor-list channel, walk the chain from `DSCPTR_NXT`: NDSIZE + 1 words a descriptor, in the order NXT, ADDRSTART, CFG,
   XCNT, XMOD. The words come from the dump, over the image's own section 7 for words nothing wrote.
4. Every register set or descriptor with WNR = 1 gives the range ADDRSTART .. ADDRSTART + XCNT x XMOD (times YCNT for 2D).
5. Check each range against the port's `dsp.l2` region.

## 2. Results

DN2 (`dn2-1.12`): six channels are on, 0/1, 10/11 and 26/27 **[D]**. Its section 7 is byte-identical to 1.11's (sha256 `336e340aa0cdcd34`).

| Channel | Writes | Note |
|---|---|---|
| DMA1 | 0x282c18e8..0x282c28e8, two descriptors of 2048 B | |
| DMA11 | 0x282c0678..0x282c0878, two of 256 B | the SPORT4 pair |
| DMA27 | 0x282c29d0..0x282c448c, two of 2748 B | the link's receive ring |

DT2 (`dt2-1.17`): DMA 0/1, 10/11, 24/25, 26/27 and 30 are on **[D]**.

| Channel | Writes | Note |
|---|---|---|
| DMA1 | 0x28263138..0x28264138, two of 2048 B | |
| DMA11 | 0x28261ec8..0x282620c8, two of 256 B | |
| DMA25 | 0x2825f848..0x28260848, 4096 B (both descriptors) | |
| DMA27 | 0x28264220..0x28265cdc, two of 2748 B | the link's receive ring (finding 04, lane G1) |
| DMA30 | 0x28268240..0x28269244, 4100 B (stop flow) | the sample path's buffer (finding 15) |

Every range is in L1 block 1. None meets `dsp.l2`: 0x2008823c..0x20100000 on DN2, 0x2001ab7c..0x20100000 on DT2.

On both devices, DMA 39/40 and 43/44 hold a CFG and no transfer. The CFG is 0x220 or 0x222: MSIZE 4 bytes, and WNR on 40 and 44.
ADDRSTART, XCNT and the current registers are 0.

## 3. L2 ECC

HRM Rev 1.1, "ECC Hardware Control" and "ECC Error Management" (pp. 8-9 to 8-11):
- After reset, ECC protection is on. The boot code initialises all L2 SRAM data and checksum cells. So a read of L2 that nothing has written
  since boot does not fail on ECC.
- A multi-bit ECC error gives a bus error and `ECC_ERR`.
- A write to an ECC bank that is not a whole aligned 32-bit word turns into a read and a write. The HRM says that this read "can create
  multi-bit errors even if the memory was initialized".
- The emulator models none of this.

What the freqshift path writes to L2 **[D]**:
- In both dumps, `L2CTL0_CTL` is 0 (every bank's ECC on) and `STAT` is 0. The values are the reset values. Nothing in stock code changes them.
- `core/dsp-link` copies a chunk into the area with whole 32-bit words: `DM(I2, M6) = R0` in `_dl_copyl`, with I2 = window base + 4 x offset.
- `fx/freqshift`'s stores are 32-bit words. Its state offsets, from `FS_PHASE` to `FS_CS`, are word offsets with a word address in the
  index register. The byte offsets are a separate set (`FS_*_B`). The source has no `(SW)`, `(BW)` or other sub-word store.

So no write on this path is a sub-word or unaligned write to L2.

## 4. What this does not cover **[O]**

- The dump has each channel's last registers and the chain they point to. A channel that runs briefly and stops before the dump is not seen.
- The DT2 run had no sample on the card (RMS 0). A sample that plays may start more transfers. The idle channels 39/40 and 43/44 are the ones to watch.
- `peripheral_model` signals completion for DMA 10/11/26/27 only and moves no data. A channel can be on in the MMRs and do nothing in the emulator.
- Other module code may make sub-word writes to L2. This finding checked `fx/freqshift` and `core/dsp-link` only.
