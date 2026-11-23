# machines service

A service of the digikit runtime that adds machine types at run time: the machine, parameter, page, click-action and native-page records of a module become rows of the
firmware's registry, the machine's voice rows are published to the DSP, and the encoder click on a machine page runs a module's callback. It also plans the sticky type numbers.
Layout and rules follow `services/shortcuts/README.md` (the template); this file lists what is specific here.

```text
src/machines.rs   the owner `Machines`: private state (`Builder`, `Slots`, `Parking`, `Applied`), `Provider` (start_in, attach, detach, settle, reload_done), `Ticker`, `Describe`
src/records.rs    the `Records` part (begin, commit, check, apply) and the free `validate` that `patchkit records --check` calls
src/dsp/part.rs   the `DspPart` over the voice-row steps of `dsp/*`; the guard's diagnostics and the voice site's attribution
src/apply.rs      `attach` (the rows of a module) and the native page record: `Applied`, the walk's state
src/build/        the builder calls (`Builder`: the collection and its gate) and the commit
src/slots.rs      the page's click state (`Slots`), with `slots/` the detector and the action lookup
src/parking.rs    the parking passes of absent numbers (`Parking`)
src/info.rs       the image table `machines_info`: layout, binding into `DKRT_MF_TAB`
dks/              the service file: `lib.rs` (the `service!` call), `hooks.rs` and `hooks-*.s` (the machine framework's hooks), `fw.rs` (the firmware calls, the page's key and turn
                  entries, `dk_param_get`, `dk_param_set`), `builder.rs` (the builder exports, which reach the owner), `rt.rs`, `gen_exports.rs` (generated)
tests/            host tests over `FakeCore`: `state.rs` (the owner), `records.rs`, `build.rs`, `assign.rs`, `guard.rs`, `file.rs` (the file's code over the cell), `contract.rs` (the
                  shared suite on both profiles through `provider_contract`, and the placeholder and parking cases over its `Lifecycle`)
```

State: the owner holds the collection of the builder calls, the click state, the parking passes and the state between `attach` and the per-record `apply`. Four tables stay named
statics of the crate (D7 of the RC3 briefs), each with its reader:

| Static | Reader |
| --- | --- |
| `DKRT_REG` (`registry.rs`) | device tests and tools by symbol; the registry rows' owners and texts |
| `DKRT_ASSIGN` (`assign.rs`) | device tests by symbol (`machines-kit`, `fx-live`); the sticky map |
| `DKRT_VROWS` (`rows.rs`) | the runtime's DSP code (`abi/dsp.rs`); the voice rows |
| `DKRT_GUARD` (`guard.rs`) | the MODULES reply, on another task than the poll that writes it |
| `DKRT_MF_TAB` (`info.rs`) | the assembler hooks (`T_*` offsets of `hooks-*.s`) |

Without an owner (before `init` made it, after `fini`) the file's calls answer the neutral way: a builder call returns null or does nothing, `dk_param_get` and
`dk_param_set` answer `E_STATE` (no flow), the key hook passes the event and the turn hook consumes nothing. The turn hook ends the click flow in the owner, then runs the page's
native handler outside it (a handler may call back into the file).

A failed `start` (no usable `machines_info` for the device) leaves nothing: the file is refused with `E_NO_REGISTRY` and the cell stays empty. ColdFire rule: an if chain, not
a `match` on dense integers, in code that runs on the device (`Describe::cap`).
