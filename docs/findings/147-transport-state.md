# Transport state words and the record predicate

DT2 OS 1.17 and DN2 OS 1.12. Static reading; no emulator run. Used by [the hot reload plan](../plans/digikit-hot-reload.md) for the refusal while the sequencer plays. Marks: **[V]** a second agent checked it against image bytes, **[D]** read once, **[O]** open.

## Play state words

- The sequencer play state is two 32-bit words. Values: 0 stopped, 1 playing, 2 stopped by STOP once (paused; MIDI continue follows). **[V]**
  - DT2 1.17: 0x43164f94 and 0x43165010.
  - DN2 1.12: 0x42c5b918 and 0x42c5b994.
- The firmware's own predicate reads both and ORs them: `move.l A,D0; or.l B,D0; rts`. DT2 FUN_400d688a (reads 0x43165010 first), DN2 0x400d86c0 (reads 0x42c5b994 first). **[V]**
- The predicate hash is `abf06de8...` (full value `abf06de823684dac9b8ac6a8a12a81eb`). It is unique on both images, and the same code runs on both. **[V]** for uniqueness; the verifier saw a 12-character prefix, so the full value is **[D]**.
- The UI treats `== 1` as "playing": TrackSwapMenuView::vfunc_4 (0x4007b694) prints PLAYBACK MUST BE STOPPED when the value is 1. **[D]**

## Writers

Start sets 1, stop once sets 2, stop and reset sets 0. **[V]** The list below is not complete.

| writer | effect | mark |
| --- | --- | --- |
| FUN_400d624a (start or continue, 1240 bytes), stores at 0x400d64cc and 0x400d6650 | both words = 1, 16 per-track words 0x43164fa0..0x43164fe0 = 1, MIDI 0xfa (start, old value != 2) or 0xfb (continue) | **[V]** the words; MIDI bytes **[D]** |
| FUN_400d7466, stores at 0x400d7490 and 0x400d7496 | both words = 2, MIDI 0xfc via FUN_40121a96 | **[V]** the stores |
| FUN_400d7988, stores at 0x400d79ba and 0x400d79c0 | both words = 0 | **[V]** |
| block at 0x400d5d62 and 0x400d5d86 (outside any known function) | stores 1, sends 0xfa | **[V]** |
| FUN_400d7b92 (4704 bytes), stores at 0x400d7ece and 0x400d80dc, clears at 0x400d860a | 1 or 0 | **[V]** |
| FUN_400d6722, clear at 0x400d673c | 0 | **[V]** |

No writer stores a value other than 0, 1 or 2. **[V]**

Callers: TransportView::vfunc_2 (0x400786ae; key 0x14 PLAY, key 0x15 STOP), the console task entry 0x400caff0, and the MIDI realtime switch (switchD_4011ece4 cases 0xa, 0xb, 0xc: start, continue, stop). Panel keys and external MIDI clock go through the same words. **[D]**

## Record predicate

- Live recording is playing AND the byte at UIStates+0x271. DT2 FUN_4005ad40, DN2 FUN_40055dec: `playing == 1 ? *(u8*)(ui + 0x271) : 0`. The hash is `dae4a446...` (full value `dae4a4462417901868a30fa7a6d6b6e0`), unique, and the pair resolves on both images. **[V]** (full hash **[D]**)
- FUN_4005ad5a(ui, v) sets the byte from the PLAY/STOP/REC key code in TransportView::vfunc_2. **[D]**
- UIStates+0x269, +0x270 and +0x272 are further record flags (FUN_4005ad8c, FUN_4005ac1e). Their meaning (grid record or arm) is **[O]**.
- The UIStates pointer comes from the existing getter `cf.window.fn.ui_states` (DT2 0x401998c0, DN2 0x4018b2a4). The object pointer sits at 0x44f370c0 (DT2) and 0x446488c8 (DN2). **[D]**

## Declaration

`cf.project.fn.transport_state` is declared in `devices/fw/project.toml` (rule `cf_func`, hash `abf06de823684dac9b8ac6a8a12a81eb`, offset 0, len 4, mark D). `patch:port` resolves it on DT2 1.17 (0x400d688a), DN2 1.12 (0x400d86c0), DN2 1.11 (0x400d855c) and Syntakt 1.42 (0x4008b838); on the two supported images the address equals the one found by reading. **[V]** (resolution by `patch:port`). `cf.project.fn.live_rec` is not declared; the reload does not use it.

Refusal for a reload: `transport_state() != 0` (refuse while playing or paused). This is a recommendation. `== 1` plus `live_rec(ui_states())` is the narrower alternative.

## Open

- **[O]** Whether state 2 (paused) keeps voices sounding. The refusal `!= 0` does not need the answer.
