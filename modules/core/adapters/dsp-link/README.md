# DSP link

`core/dsp-link` is a framework module: it lets the digikit runtime send bytes (DSP code and data) to the DSP while
the device runs. Nothing changes for the player. Emulator only.

- Firmware: Digitakt II 1.17 and Digitone II 1.12 (the same anchor rules on both).
- What it adds: a hook on the ColdFire's per-block frame sender (to put a chunk in the unused end of the frame) and a hook
  at the end of the DSP's block handler (to take the chunk, copy it into a reserved area of free L2 (512 KiB on the Digitakt II, 320 KiB on the Digitone II), and
  acknowledge). The ColdFire interface is `dl_send(dst_l2, buf, len)`, `dl_send_win(win, dst_word, buf, len)` (window 1: the voices registry of
  `core/dsp-voice`, window 2: the effect rows of `core/fx`, one word at a time) and `dl_poll(status)` (C ABI).
- Speed: about 960 KB/s on the Digitakt II and 72 KB/s on the Digitone II; with no transfer it costs the DSP about 90
  instructions per block (about 0.04 % of a block's cycles: 24 for the link itself, 23 to pin and restore M5, M6, L2 and L3, 43 for
  the diagnostic words below and the record addresses; findings 45 and 48).
- Diagnostics (the fault table): `int dl_diag(u32 *out, u32 n)` (ColdFire C ABI, optional link) returns the DSP's diagnostic words of the last
  block, 32 words (24 before runtime 0.5.3, 17 before runtime 5), or a negative number when there are none (-1: no record address in the image;
  -2: the reply carries no diagnostic block, or the DSP has stopped writing it). The link fills the table in every image, with or without
  `core/dsp-voice`; the words of a site that is not in the image stay 0. The words travel in the DSP's reply record, two per slot row (rows 1 to 16, bytes
  +0x44 and +0x48 of the row, the entries 11 and 12 that no DSP code writes); the exact offsets are in `src/dsp/dsp-link.inc`, and
  `tools/patchkit/pk_ctxprobe.py` reads them in the emulator. The record has 32 rows, so no row is added; on the Digitone II these bytes are samples of
  the effect buses (finding 31), and rows 13 to 16 add four to the twelve used before. Layout:

  | word | content |
  | --- | --- |
  | 0 | `0xD1A6` in the low half, the word count (32) in the high half |
  | 1 | blocks that ran the link's hook (a heartbeat of the block handler) |
  | 2 | blocks that reached `core/dsp-voice`'s hook (its heartbeat) |
  | 3-8 | M5, M6, M7, M13, M14, M15 |
  | 9-13 | I6, I7, B7, L7, MODE1 |
  | 14 | the voice site's `last`, the breadcrumb: stage in bits 0-7, voice (0-31) in bits 8-15, machine type in bits 16-31 |
  | 15 | dispatches to registered machine types since the start |
  | 16 | routines that returned (15 minus 16 is 1 while a routine runs; a core stopped inside one leaves it at 1) |
  | 17 | the guard of `core/dsp-voice` (finding 51): faults counted so far (low 16 bits) \| kind of the last fault << 16 (1 Inf or NaN, 2 over the level bound) \| machine types disabled << 24 |
  | 18 | type \| voice << 16 of the last fault |
  | 19-22 | the eight machine parameter words of that call (clamped, as the routine saw them), knob 2k in the low half, knob 2k + 1 in the high half of word 19 + k |
  | 23 | the machine type the guard disabled last |
  | 24 | the effect site's `last` (`core/fx`): stage (4 the core is about to call the module, 5 it returned), site << 8 (2 master route, 3 send route), the row's byte offset in the effect table << 16 |
  | 25 | the module step: what the module stored into `_dk_step` (`DK_STEP`); the core clears it right before each call into a module |
  | 26-27 | module calls of the master route and of the send route |
  | 28 | effect faults (low 16 bits) \| the row of the last fault (byte offset + 1) << 16 |
  | 29-30 | the addresses of this block's reply record and of the one before |
  | 31 | the effect row switched off last (byte offset + 1) |

  Words 3-13 are the registers as the firmware left them at the first voice of the block, before the wrapper pins anything. A frozen heartbeat is a halted
  core. The breadcrumb names the step of a machine's path that `core/dsp-voice` reached last: 1 hook entered (every voice; the type is not known yet and
  the field is 0), 2 type registered, 3 stock path taken for a type of 7 or more that no module registered, 4 routine about to be called, 5 routine returned,
  6 the odd voice's copy done, 7 epilogue done. The wrapper writes it into the reply record at once (both reply records, header and dispatch count with it),
  not only at the end of the block, so that it is there when the core stops inside a routine and never reaches the block's end: the DMA that sends the record
  does not need the core (finding 48). The effect site (word 24) and the module step (word 25) are written the same way. On the Digitone II the voice wrapper
  writes only the stages 4 and 5 (the slot is the voice) and words 15 to 23 are 0 (the dispatch counts and the guard belong to the Digitakt II wrapper; the
  Digitone II wrapper only scans rows of the new size).
  The module step needs a place the ColdFire can read after the core has stopped. `core/dyndsp` exports `_dk_step` (a 32-bit DM word of this module, the
  static fallback, copied into word 25 at the block tail). The runtime points the stores of a module's DSP image at word 25 of the older reply record
  instead (the record addresses are words 29 and 30), so that a step stored just before the DSP stops is still in the record. The core clears word 25 in both
  records before each call; the module writes into one, so the ColdFire takes the non-zero value of the two replies. Finding 77 has the design.
- Limits: the destination must lie in the module's area (`_dl_area`); the DSP code that runs from it must be linked for
  that address or relocated by the runtime (`docs/design/digikit-dkm.md`, "DSP section"). The emulator does not model the DSP caches, so the cache invalidate after the last chunk has not run on
  a device.

Design, measurements and open points: [docs/findings/31-dsp-link.md](../../../../docs/findings/31-dsp-link.md) and, for the windows and
the wider chunk header, [docs/findings/32-runtime-dsp-machines.md](../../../../docs/findings/32-runtime-dsp-machines.md). Proof:
`uv run python -m tools.patchkit.pk_dsplink proof --device dt2|dn2`; tests: `tests/patchkit/test_dsplink.py`.
