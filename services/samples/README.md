# samples service

A service of the digikit runtime that gives a module its sample pool: the nine `dk_sample_pool_*` calls (`sdk/include/dk/platform/`), per-track stereo samples bound to a
backing adapter (`image-modules/points/sample-pool`) and bounded peak windows for a picker. A module that has a track table in its DSP section (version 5, record kind 2) owns
one client slot; its tracks are published to the DSP by queued link writes. It follows the template of [services/shortcuts](../shortcuts/README.md): one owner object, its
concepts beside it. It differs from it in having no record kind (so no `Records` part) and a `DspPart`.

```text
src/samples.rs   the owner `Samples`: private state, the module API that needs the object as methods (`open`, `peaks`, `peaks_window`), `Provider` (start_in), `DspPart` and `Ticker` in one block each
src/table.rs     `PoolTable` in the cell `POOL_TABLE` (outside the owner): what the pool calls read and write at once, and the bodies of those calls
src/tracks.rs    `Tracks`: the clients' slots and their state machine (`dkrt_sdk::sample_tracks`), bound to the adapter and to the table writes (`Device` is its environment)
src/writes.rs    `Writes`: the queued table writes over the DSP link (close, publish), one in flight at a time, and when a client's table may be written
src/pool.rs      `Pool`: the adapter's calls, found through its table `sample_pool_info` (`Pool::from_info`) or given by a host test
src/peakwin.rs   `PeakWindows`: one window per client (state, in the owner); `PeakJobs`: the jobs' data, one cell per client, outside the owner
src/files.rs     the sample root of the device and the identity of a file (`probe`)
src/dsp/mod.rs   check, admit and claim of the track-table record of a DSP section
src/lib.rs       docs, `mod` and `pub use` only
dks/             the service file: `lib.rs` (the `service!` call), `glue.rs` (the C entry points and the two pool event slots, which reach the owner), `gen_exports.rs` (generated)
tests/           host tests over `FakeCore`: `dsp.rs` (admit, claim, loaded, release, tick, the owner made in a dirty slot), `contract.rs` (the shared suite on both profiles),
                 `file.rs` (the service file's own code over the cell), `stubs/` (the `dkc_*` imports as host functions). Unit tests sit beside the code in `src/*/tests.rs`.
```

What goes where:

- The owner holds all state as private fields. The file's cell (`DK_SVC_OWNER`) holds the one instance: `Provider::start_in` makes it in the file's `init`, in place (zero bytes plus the
  few non-zero starting values), because the track core is 35 kB. The exports and the event slots reach it with `DK_SVC_OWNER.with(...)`; with no owner they answer `E_CONTEXT`
  (a call) or drop the event.
- The pool calls `dk_sample_pool_add`, `_set`, `_clear`, `_status`, `_info` and `_path` do not enter the owner. They come from the MIDI task and the UI task, and the owner is busy exactly
  when the UI task is inside a tick or a window, so the guard would refuse them. They read and change the pool table (`table.rs`, the static `POOL_TABLE`, a `Shared` cell: sections with
  interrupts masked): per entry the status, the stored path with its 32-bit key, the info of a READY entry, and per client whether it is open. The results are final when the call returns
  (add: the index of the first free entry or of the entry that holds the path; set: 0 or an error; clear: 0, the entry is a hole), and the next `status` shows them. A call leaves a
  request; `Tracks::intake` takes the requests at the start of the tick (also before a loss of the backing) and applies them as `select` and `unbind`; `Tracks::mirror` publishes the status
  and info of each entry after the tick, a loss, a claim and a release, never over an entry with a request waiting. `dk_sample_pool_open`, `_peaks` and `_peaks_window` stay in the owner
  (UI task only). The table is not a field of `Samples` (it holds a reference to the static); a host test gives its own cell.
  Longest sections: `add` scans 16 entries by 32-bit key and compares the bytes of at most one stored path (one of 255 bytes at most, where keys are equal), then copies the path in (16 compares, one 255-byte compare, one 255-byte copy: bounded by that count
  of steps, about a thousand ColdFire cycles at most, a few microseconds; an estimate, not timed on the device). The old code held the mask over the byte compare of every entry of equal length (up to 16 x 255
  bytes). `publish` stores 32 entries (copied outside the section), `take_request` scans 32 entries and copies one path.
- Named statics stay only where something reads them by symbol: `DKRT_SMP_RECORDS` and `DKRT_SMP_CONTROL` (the L2 addresses of the 16 records and 16 control sets of each client;
  read by the device tests of the shot machine). `Tracks::reserve` is the only writer; the logic reads the copies in `Writes`.
- `Ticker::tick` runs in the early phase only. The adapter's `dkpool_tick` is the part's pre-tick step (`Ticker::pre_tick`): the table's tick function runs it first, before the owner
  is entered. It fires the pool events, which reach the owner as typed events (`events.rs`, the `Reentry` part: `PoolEvent::Done`, `Lost` and `PeakDone`): with the owner idle
  `DK_SVC_OWNER.post` runs each at once, so the completions apply before the writes pump and the track step of the pass, as before the owner had a guard. `tests/dsp.rs` covers the order.
- A module goes down through `DspPart::release`: its tracks are unbound without closing them in the DSP, the pool references are released, its peak window is stopped and its client
  slot is free. A write already in flight ends by itself.
- A peak-window job holds no pointer into the owner. Its data (request, result, cursor) is a `PeakJob` cell of the static `DK_PEAK_JOBS` of the file, and a phase word hands it
  between the UI task and the job task (`Idle`, `Posted`, `Running`, `Done`; the UI task touches it in `Idle` and `Done`, the job task in `Posted` and `Running`). The job's end
  (`peak_done`, UI task) keeps the status in the cell and posts a `PoolEvent::PeakDone` to the owner, which changes the window under its guard.
- Tests: logic of the tracks, the writes and the windows in unit tests next to the code; the part calls in `dsp.rs` over one owner per test (no lock); the shared suite in `contract.rs` through `provider_contract`.
  A test that needs the adapter gives the owner a `Pool` (`pool::refusing()` and `..` struct update for the one function it watches).
