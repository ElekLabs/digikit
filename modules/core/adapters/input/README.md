# core/input

Presses keys and turns encoders from software: `dk_input_key(code, down)` and `dk_input_turn(encoder, delta)`, imported by name by a run-time module
(`digikit/remote`). Both call the firmware's own panel decoders, the functions the UART packet handler calls, so the events are the ones a real
panel produces.

- **Device-agnostic.** The key code layout and the backend come from the device file (`[input] backend`, `[panel]`). The backend `panel-uart` is the
  panel MCU path of the Digitakt II and the Digitone II; another device with another path adds a backend in `src/cf/input.c`.
- **Anchors by hash.** The two decoders and the literal of the previous-mask table are `cf_func` rules (the cfdb `h_strict` hash), resolved per port.
- **Calls.** Plain C ABI (stack arguments, the result in d0), made with SR masked to 0x2700 from the MIDI task. A key reaches the decoder only when it
  changes the mask of its channel. The UI queue is checked for room first.
- Facts: `docs/findings/58-panel-input-path.md` (marked [D] until a second check).
