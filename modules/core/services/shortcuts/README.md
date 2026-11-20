# shortcuts service

A service of the digikit runtime that lets a module bind a key combination to a function. A combination is two logical keys: the first held, then the second pressed
(`dk_shortcut_add`, `sdk/include/dk/platform/shortcuts.h`). Key names are the SDK's (`DK_KEY_*`, `DK_KEY_TRIG(n)`), not panel codes.

```text
src/records.rs    the Service handlers: declare, validate, check, apply_record, remove, tick
src/shortcuts.rs  the state machine: one key event in, the verdict (pass or swallow) out
src/bindings.rs   the binding table and the callbacks
src/keys.rs       which keys are held
src/slots.rs      what the service fills in the firmware
dks/              the service file: the key-dispatch hook (tail mode) and its assembler
tests/            host tests over FakeCore (state, records, contract); device test in tests/device
```

How it works:

- A module's `init` adds `SHORTCUT` records (id, label, two keys, `on`, `off`). The first owner of a pair of keys wins: a second module that asks for the same pair gets a
  clash log (`SHORTCUT_CLASH`) and its binding is not active.
- A binding whose key the device has no panel code for is kept but inactive (`SHORTCUT_INACTIVE` is logged); it works on a device that has the key.
- The hook sees every key event before the stock firmware. The first key is never swallowed (the firmware needs it). When the second key completes a binding, that press,
  its repeats and its release are swallowed, so the stock screen never sees them; every other event passes, so [TRK]+[TRIG] still selects a track.
- The hook does not call modules. It queues `on` (when the binding completes) and `off` (when either key is released); the service's tick runs the queue in the UI task.
- When a module unloads, its bindings and its queued calls are removed (a held binding gets its `off` first).

The neutral crate exports only `dk_module` and `DKRT_*` state symbols, names no firmware, and has no assembler; the hook and the site come from `devices/fw/input.toml` and the port files.
