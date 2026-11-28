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
- **Key subscribers.** The module also owns the firmware's key walker (`cf.input.site.key_event_walk`; the devices that have it): its hook (`src/cf/key-walk.s`) offers each
  key event of the panel, in the UI task before any view, to up to `KEY_SUBSCRIBER_SLOTS` (4) subscribers in slot order. The first that returns nonzero consumes the event;
  otherwise the stock walker runs. A service subscribes with `key_subscribe(fn)` and leaves with `key_unsubscribe(fn)` through the image's table `input_keys_info`
  (`'KEYS'`, version 1, slots, subscribe, unsubscribe; `src/cf/key_subscribers.h`), both in the UI task; the `shortcuts` service does so at start and stop. Tests: `tests/key-subscribers-host.c`
  (run by `tests/key-subscribers.test.ts`: slots and walk) and `packages/patchkit/tests/key_subscribers.rs` (the hook and the info words of the committed object on the ColdFire core).
