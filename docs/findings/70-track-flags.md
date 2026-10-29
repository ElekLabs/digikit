# 70. Per-track flags: storage, frame word and FUNC + knob press (DT2 1.17, 2026-10-28)

The facts behind `modules/core/trackflags`. Marks: **[D]** read or measured once by the author; nothing here is **[V]**.

## No free saved and sent parameter word **[D]**

- The machine's eight engine slots (mirror 25-28, 31-34) are all taken on a sample-based machine of ours: TUNE and SAMP stay on
  knobs A and D, the machine's six parameters take the rest. Every other Sound index from 1 to 68 belongs to a stock owner (LFOs 1-24,
  filter, amp, sends, portamento, track FX).
- Indices 69 and 70 are free but neither saved nor sent (finding 64); core/fxpages uses them for the send knobs.
- Index 29 (BANK) is saved (save table `0x40213a50`, 1.17: the tables moved by +0x410 from finding 02's 1.16 addresses) and sent,
  but it is the sample's bank: frame index 30 carries `SAMP << 8`, 29 the bank. A direct write to `Sound[29]` reaches neither the
  frame (the frame copies the smoothed mirror, which only the parameter-change path updates) nor a reload.

## What core/trackflags uses **[D]**

| What | DT2 1.17 | Notes |
| --- | --- | --- |
| storage | `Sound[0]`, `Sound + 0x14` | a dummy runtime index; the stock saver `FUN_400dba94` writes it to record slot 0 (save table entry 0 = 0); the loader `FUN_400db3b2` writes the padding slots 0, 4 .. 32 into it, so slot 32 (0) wins: a hook after the table loop (`0x400db450`) puts slot 0 back. Stock firmware loads slot 0 into nothing |
| the live Sounds | `*0x80004704 + 0x34 + 0x450 t` | the table of Sound pointers at `0x8000470c` is cleared while a machine changes (read 0 from the YES in the machine list until the next trig) |
| frame word | TX `+0xf8 + 0x60 t` (DSP frame copy, the low half of the 32-bit word at that byte) | one of the six words `vector_191_handler` writes only for type 6 (`0x4002ec18`, MANUAL SLICE's slice boundaries, and core/machines' clones of it); the hook writes it after the per-track loop (`0x4002f344`) for tracks whose machine has a toggle row |
| key | `MachineParameterPageView::vfunc_2` (`0x4005fb0c`) | key id at event `+0xc`, knob pushes `0x29`..`0x30`; a press has bit 0 set and bit 3 clear at `+0x10`; FUNC held is `FUN_4011e150(0x11)`; the active track `FUN_4004abb8(view + 0x74)`; a header message `FUN_4005b23a(view + 0x6c, fmt, ...)` |

Emulator (ChordWT on track 1, a machine since retired; ParaWT has the same row): FUNC + press B shows `SMOOTHING OFF` / `SMOOTHING ON`, flips `Sound[0]` bit 0, and the DSP frame
copy follows; FUNC + press C (no row) changes nothing; Save Kit with the bit set, the bit cleared, Load Kit: the bit is set again.

The page's stock knob-press path (`ParameterPageView::vfunc_2`) opens the LFO destination list for parameters whose row word `+0x24`
has bits `0x70000` (only the three LFO DEST codes); the `Func+Press to enable` text sits in the turn handler. Neither is a generic
enable flag.
