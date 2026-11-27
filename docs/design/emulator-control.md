# Emulator control protocol

> User reference for what is implemented: docs/CONTROL.md. This document is the design and includes parts that are not built.

Status: P0, P1 and the injection part of P3 are implemented (2026-10-07); P2
and the rest of P3 are design. Scope: the desktop app (`mise run emu`), the
headless daemon `digi-emud` (`mise run emu:serve`), the Python client
`tools/control/digi_client.py` (`mise run emu:ctl`), and later the browser worker.
Marks: **[V]** read in the code for this design, **[O]** open.

## What is built

Crate `packages/emulator/control` (`digi-control`): `proto` (JSON-RPC types, error codes,
no I/O), `controller` (the former desktop `actor()`; one owner thread, jobs and
run modes), `dispatch` (one request object in, one response out: the
transport-independent entry a browser worker feeds too), `server` (Unix socket,
discovery file; feature `server`, unix only), `firmware`, `events`, `cli` (the
command line the desktop app and the daemon share). The desktop app keeps its
Tauri commands; they are messages to the same controller. The socket is on in
the desktop app unless `--no-control` (question 3: default on).

Methods now served: `hello`, `status`, `firmware.load`, `restart`, `run.mode`,
`step`, `run_until`, `input.button/turn/tap/schedule`, `inputs.recorded`,
`framebuffer.get` (raw 1024-byte panel bytes; the client makes the PNG),
`diagnostics`, `audio.report`, `state.digest`, `pcm.tap` (start/report PCM
level, window RMS and SHA-256 of the DSP output), `dsp.patch` (inject a patch
file's text), `dsp.peek` (`addr`, `len` up to 64 KiB: DSP memory as hex, `??` for a
byte the engine does not know), `events.subscribe`, `quit`. Events: `session.changed`,
`firmware.loaded`, `mode`, `scenario.done`, `lagged`. Not built yet (P2):
captures, `mem.*.read`, `state.save/load`, `scenario.run`, `replay`,
`dsp.counters`.

Differences from the text below: `input.tap` schedules the press and the
release (returns `scheduled_at` and `release_at`) instead of being a job; the
UI session id is kept when a control request replaces the runtime (the webview
keeps stepping the new one) and a separate `epoch` counts replacements;
`firmware.load` accepts `dsp_patch` (a patch file applied on top of the .syx);
the daemon refuses `--scenario`. `emu_step` returns the status without
stepping while a job runs or the mode is not `ui`; a frame a control chunk
consumed is handed to the webview's next call.

## Goals and non-goals

Goals
- Drive a running emulator end to end from Python: lifecycle (load or inject a
  .syx, patched ones included; choose DSP inputs and snapshot; restart; pause
  and resume; step N instructions; save and load state; quit), input (button,
  turn, tap, scenarios, replay of recorded sessions), observation (status,
  audio report, DSP tier counters, framebuffer, CF and DSP memory peek,
  captures: PCM WAV, device-output WAV, DSPI frame log, DSP memory watch), and
  an event stream (underruns, halts, frames, inputs).
- Keep determinism: inputs land at exact ColdFire instruction counts; every
  session can be replayed.
- No cost in the hot loop: control work happens at chunk boundaries
  (≤ 250,000 instructions apart).
- One method set for every host (desktop app, `digi-emud`, browser worker);
  only the transport differs.
- A short loop for patched firmware: inject the .syx, run a scenario, compare.

Non-goals: remote access (local only); a GUI for the protocol; wall-clock
exactness (playback pacing and sink statistics stay host-dependent); a new
async runtime.

## Constraints found in the code

Written before the build: items 7, 8 and 10 are now handled by `firmware.load`
(see Firmware injection).

1. `CoupledRuntime` is `!Send` (`Rc<RefCell<Shared>>`, `ThreadedHandle` on
   `Rc<RefCell<Link>>`): one owner thread builds and uses it, as the Tauri actor
   does today. **[V]**
2. A step is a chunk of at most 250,000 instructions; exact event timing works
   by clipping the chunk (`coupled_run.rs`, desktop `--scenario`). **[V]**
3. Inputs are recorded with `at_instr` and the DSP frame; `recorded_scenario`
   replays them with `exact_events`. **[V]**
4. Frame emission has one consumer: `snapshot()` returns a frame once per
   revision. A remote `snapshot()` would steal frames from the webview. **[V]**
5. `snapshot()` and `diagnostics()` sync the DSP pipeline; `audio_status()`
   only polls. Status polling must use the polling path. **[V]**
6. Captures are configured only at session start (`DIGI_EMU_PCM_F32LE`,
   `DIGI_EMU_SINK_WAV`, `DIGI_DSPI_LOG`, `DIGI_DSP_WATCH*`). The worker has no
   peek or diagnostics message, but `DspEngine::peek_word` exists. **[V]**
7. A patched .syx cannot be loaded today: `Emulator::new` identifies firmware
   by the SHA-256 of the whole .syx, then the MAIN digest; any byte change gives
   `UnknownFirmware`. **[V]**
8. `load_state` needs a fresh emulator and refuses a snapshot whose MAIN
   SHA-256, device or version differ: a CF snapshot survives a patch outside
   MAIN and fails on one inside it. **[V]**
9. Coupled audio is limited to DN2 1.11; with `--auto-coupled` a firmware whose
   bytes differ from the profile .syx silently loses audio. **[V]**
10. The fast-tier tables are keyed by the .syx SHA-256, so a patched .syx gets
    no fast regions even when its DSP code is unchanged. **[V]**
11. Costs: CF snapshot restore 1.9 s vs cold CF boot 33 s; the fresh DSP state
    cannot be rebuilt; a coupled cold boot attaches the DSP from the first
    frame; the generated core rebuild takes about 4 min.
12. In `--scenario` mode the desktop app exits when the scenario is done; a
    controller must emit an event instead. **[V]**

## Architecture

```
 Python / CLI ──NDJSON-RPC──▶ socket thread(s) ──mpsc<Request>──▶ Controller (owner thread)
 Tauri commands ─────────────────────────────── mpsc<Request> ──▶   owns CoupledRuntime
 browser worker ──postMessage──▶ same dispatcher (wasm, no threads)     │
                 ◀── events: bounded channel, drop-oldest + counter ─────┘
```

- New crate `packages/emulator/control` (`digi-control`):
  - `proto`: request, response and event types (serde), `SCHEMA_VERSION`,
    error codes; no I/O; builds for wasm.
  - `controller`: today's desktop `actor()` moved here; owns
    `Option<CoupledRuntime>`, the session id, the cached firmware, the input
    schedule, the run mode, captures and the event sink.
  - `server` (feature `server`, unix only): Unix-socket listener, one thread
    per client, discovery file.
- The desktop app keeps its Tauri commands; they send `Request`s into the same
  controller channel. With `--control` it also serves the socket.
- `digi-emud`: controller plus server, no webview; desktop CLI flags;
  `--paused` by default.
- `coupled-run` stays the batch benchmark (open question 6).

### Run modes and stepping

| Mode | Who advances the guest | Use |
|---|---|---|
| `ui` | the webview pump's `emu_step` | desktop default |
| `free` | the controller, chunk after chunk | daemon running live, soaks |
| `paused` | nobody; `emu_step` returns status without stepping | scripted tests |
| job | `step`, `run_until`, `tap` as resumable jobs, one chunk per turn | remote stepping |

- A job puts the controller in `paused` for its length; commands arriving
  meanwhile are served between chunks, so the UI stays live. When the job
  ends, the previous mode returns unless the request set `then: "paused"`.
- Per chunk: fire due scheduled inputs; clip the budget to the next
  `at_instr`; `step_chunk`; collect cheap observations; emit events.
- Frame events and `framebuffer.get` read a new non-consuming
  `Emulator::current_frame()`; only the UI path calls `snapshot()`.
- `load`, `restart` and `state.load` raise the session id and emit
  `session.changed` (UI reload behaviour **[O]**).

## Transport, framing, discovery, auth

- JSON-RPC 2.0, one object per line (NDJSON), over a Unix stream socket.
  Events are notifications (`{"jsonrpc":"2.0","method":"event","params":{...}}`)
  sent only to clients that called `events.subscribe`.
- Binary data: base64 in results (framebuffer, peeks). Large data (PCM, logs)
  goes to files whose paths the client chooses.
- Socket `out/native/run/<pid>.sock` (dir 0700, socket 0600). Discovery file
  `out/native/run/current.json`: `{pid, socket, schema_version, host, syx_sha256,
  started}`; stale entries pruned on start. macOS limits socket paths to 104
  bytes: fall back to `$TMPDIR/digi-<pid>.sock` and record the real path.
- Auth: filesystem permissions. No TCP in phase 1; a later 127.0.0.1 WebSocket
  needs a token from the discovery file.
- `hello` returns `{schema_version, host, features, methods}`; additive changes
  keep the version; clients send `min_schema`.

## Methods (schema_version 1)

Every result includes `session`, `icount` (absolute ColdFire count) and
`elapsed` (count since ready). `[]` marks optional params.

| Method | Params | Result / notes |
|---|---|---|
| `hello` | `[min_schema]` | versions, features, methods |
| `status` | | cheap status + mode, job, audio summary; never syncs the DSP |
| `firmware.load` | `syx`, `[base_sha256]`, `[start: auto\|snapshot\|cold]`, `[dsp: {image,state,snapshot}\|"none"\|"default"]`, `[card]` | `{session, start_used, main_changed, sections_changed, snapshot_ok, seconds}`; see Injection |
| `restart` | | same inputs, new session |
| `run.mode` | `mode: ui\|free\|paused` | previous mode |
| `step` | `instructions`, `[then]` | runs exactly N (job) |
| `run_until` | one of `elapsed`, `frames`, `audio_s`, `[timeout_wall_s]` | status; `timeout` keeps the reached state |
| `input.button` | `code`, `down`, `[at_instr]` | `{scheduled_at}` |
| `input.turn` | `encoder`, `detents`, `[at_instr]` | same |
| `input.tap` | `code`, `[hold=40e6]`, `[at_instr]` | job: down, hold, up |
| `input.schedule` | `events` (scenario event objects) | merged into the schedule |
| `scenario.run` | `name` \| `file` \| `object`, `[report_path]` | job; `scenario.done` event with the report |
| `inputs.recorded` | | `recorded_scenario` |
| `replay` | `file` \| `object` | restart, then `scenario.run` with `exact_events` |
| `state.save` / `state.load` | `path` | `.snap` + `.snap.dsp`; load rebuilds the runtime, new session |
| `diagnostics` | | heavy (syncs the DSP) |
| `audio.report` | `[full_events]` | `audio_status()` |
| `dsp.counters` | `[exact]` | `live_report` (no wait); `exact` waits for the pipeline |
| `dsp.profile` | `region\|execution`, `on` | |
| `framebuffer.get` | `[format: raw\|png]` | `{revision, width, height, data_b64}`; non-consuming |
| `mem.cf.read` | `addr`, `len ≤ 64 KiB` | new side-effect-free `Emulator::peek` |
| `mem.dsp.read` | `addr`, `words ≤ 4096` | new worker `Msg::Peek`, ordered after frames in flight |
| `capture.start` | `kind: pcm\|sink\|dspi_log\|dsp_watch`, `path`, `[seconds]`, `[watch]` | `{capture_id}` |
| `capture.stop` | `capture_id` | `{path, count, error}` |
| `captures` | | active captures |
| `events.subscribe` | `[kinds]`, `[frame_every]` | notifications follow |
| `quit` | | flushes captures, exits |

Errors (JSON-RPC `code`): `-32602` bad params, `1` no firmware, `2` stale
session, `3` busy, `4` firmware rejected, `5` audio unavailable, `6` guest
fault, `7` DSP halted, `8` I/O, `9` timeout; `data` carries details.

Events (`params = {kind, session, icount, elapsed, ...}`): `frame` (revision
only), `input`, `underrun`/`resume`, `halt`, `fault`, `capture.stopped`,
`scenario.done`, `session.changed`, `mode`, `lagged` (`{dropped}`, bounded
queue of 1024 per client). Audio and halt state are checked at most every
100 ms of wall time via the polling path.

## Captures: from env vars to methods

The env vars stay as session-start defaults. `capture.start/stop` add run-time
control: `pcm` via `AudioSession::capture_pcm` plus stop; `sink` needs start
and stop on `PcmPlayer` **[O]**; `dspi_log` and `dsp_watch` via a new worker
`Msg::Diag` so files open and close in frame order. Each capture writes a
`.json` with its start/stop `icount` and DSP frame for alignment with replays.

## Firmware injection

`firmware.load {syx, [base_sha256], [start: auto|snapshot|cold], [card],
[dsp_patch]}` compares the new .syx with the running one section by section
(decoded SHA-256) and picks the cheapest way (**built**):

1. Same MAIN, DSP stream changed (the dev loop): no restart. The two boot
   streams are loaded as memory (`sharc_peer::dsp_stream`, the Rust
   twin of `tools/sharc_dev/sharcldr.py`), the changed bytes become `BYTES` patch
   operations, and the DSP worker applies them between two frames
   (`Msg::Patch`; the report carries `at_frame`). The engine's code-identity
   checks do the rest: generated blocks of regions that touch a changed range
   are barred (per range, lane B), the fast tier re-verifies its code words.
   Report: `start_used: "hot-dsp"`, `sections_changed`, `dsp.changed_bytes`,
   `dsp.ranges`, `seconds`. Without a DSP audio session the change is reported
   and not applied.
2. MAIN is the base's again (or the file is the base): rebuild from the base
   .syx and restore the CF snapshot and DSP state (`start_used: "snapshot"`,
   about 0.2 s with the ready inputs), then re-apply the DSP difference.
3. Patched MAIN: `Emulator::new_patched(syx, base_sha256)` identifies the
   firmware by the base's hash (no whole-file or MAIN digest check; each
   signature lookup still has to resolve). With the launch's DSP inputs (a
   coupled daemon or app) the ColdFire cold boots with the base's DSP
   attached (`cold::boot_patched_to_ready` on a worker thread; the
   controller answers `firmware.load` when it is ready) and the ready pair
   (CF snapshot and DSP state) is cached at
   `<run_dir>/patched/<MAIN sha16>-<key>/`, where the key hashes the MAIN
   digest, the DSP image, the cold-idle state and the card file. The next load
   restores it (`start_used: "snapshot-cache"`, 0.3 to 0.5 s) with DSP audio;
   a DSP section that differs from the base's is applied hot afterwards.
   `start: snapshot` never boots cold (error 4); `start: cold` rebuilds the
   pair. Uncached: 6 to 7 s (DN2 1.12) and 13 s (DT2 with a card), see
   finding 17. The ready inputs must belong to the base: `SOURCE.json` next
   to the DSP image names their .syx, and a mismatch (or no inputs, or no
   `cold-idle.dsp`) falls back to the old ColdFire-only boot, saved at
   `<run_dir>/patched/<MAIN sha256>.snap`, with `coupled_note` saying why.
   `auto_no: true` (also on `restart`) presses NO once the new runtime is
   ready, closing DT2's FILE SYSTEM OK modal as `card.load` does.
4. `base_sha256` is optional: it defaults to the running firmware's base and
   must match it when given. The first load of a patched file needs it.

A patched .syx comes from `tools/patchkit/dsp_patch.py syx BASE.syx PATCH OUT.syx`
(applies a patch file to the DSP stream and rebuilds the container with
`tools/firmware/build.py`, store-only packing so the file is about 3x larger), or the
patch file goes straight in with `firmware.load {syx: BASE, dsp_patch: FILE}`.

Not built: a hot ColdFire code overlay. It is only safe when the changed MAIN
bytes are code that is entered afresh (not on a task's stack or PC, not
init-time code that already ran) and no data layout, signature site or address
the controller resolved at construction moves; it cannot be told from the bytes
alone. A possible `start: overlay` would diff MAIN, refuse changes outside
function bodies, write them into guest RAM at a chunk boundary, drop the
decode and fused-loop caches, and be checked against the cold-boot digest of
the same patch. **[O]**

## Determinism

- Same firmware, start state and inputs at the same `at_instr` give the same
  CF state digest, DSP export and PCM SHA-256 (threaded = sync peer; gated by
  `coupled_ready_exactness`).
- Scheduled inputs fire exactly at `at_instr` (clipped chunks); immediate
  inputs fire at the current boundary and are recorded with that count, so
  `inputs.recorded` replays exactly. `at_frame`/`audio_s` are deterministic.
- Not deterministic: wall time, real-time factor, the sink (device output,
  underruns, sink WAV), event delivery time, and the count at which an
  immediate input lands while the UI free-runs.
- Reads at a boundary have no side effects; `diagnostics` and exact counters
  cost wall time only.

## Python client (`tools/control/digi_client.py`)

Standard library only; on the `CLEAN` and `TYPED` lists.

```python
class Emu:
    @classmethod
    def connect(cls, socket: str | None = None) -> "Emu"      # reads current.json
    @classmethod
    def spawn(cls, *args: str, paused: bool = True) -> "Emu"  # starts digi-emud; context manager
    def call(self, method: str, **params) -> dict             # raises EmuError(code, message, data)
    def events(self, *kinds: str) -> Iterator[dict]
    # wrappers: status, load, step, run_until, button, turn, tap, schedule,
    # scenario, recorded, replay, save, restore, audio, counters, frame,
    # cf_read, dsp_read, capture (context manager), quit
```

Tap test with a WAV and a badness check:
```python
with Emu.spawn("--auto-coupled") as emu, emu.capture("pcm", "out/native/runs/tap.wav"):
    emu.tap(12, hold=1_000_000, at_instr=0)          # NO first
    emu.tap(25, hold=40_000_000, at_instr=2_000_000)
    emu.run_until(audio_s=8.5)
    a = emu.audio()
    assert a["missing_blocks"] == 0 and a["halted"] is None
check_wav("out/native/runs/tap.wav", min_rms=1e-3, max_jump=0.5)
```

A/B of a patched .syx:
```python
def render(syx):
    with Emu.spawn(paused=True) as emu:
        r = emu.load(syx, base_sha256=STOCK_SHA, start="auto")
        emu.scenario("trig1-tap-release")
        return r["start_used"], emu.audio()["pcm_sha256"]
print(render("Digitone_II_OS1.11.syx"), render("out/patch/dn2-p1.syx"))
```

Soak with underrun events:
```python
with Emu.spawn("--auto-coupled", "--audio-buffer", "1") as emu:
    emu.call("run.mode", mode="free")
    for ev in emu.events("underrun", "halt", "fault", "lagged"):
        log(ev)
        if ev["kind"] != "underrun":
            break
```

## mise tasks

- `emu:serve`: `tools/emulator/native_emu.py --daemon` (builds `digi-emud` with the same
  target dir, PGO and fast-env rules as `emu`).
- `emu:ctl`: `uv run python -m tools.control.digi_client` CLI (`status`, `tap 25`,
  `load FILE`, `capture pcm out.wav --seconds 10`, `events`).
- `emu` gains `--control` (default on: question 3).

## Test strategy

- Rust (`digi-control`): protocol round-trips; controller tests on a CF-only
  runtime (skipped without the DN2 .syx); job interleaving (`status` during
  `run_until`, a UI `emu_step` while paused); exact `at_instr`; non-consuming
  frame reads.
- pytest (`tests/control/test_digi_client.py`): spawn `digi-emud --cf-only`; `hello`,
  version gating, `step` exactness, record-then-replay to the same state
  digest, errors, quit.
- `slow`: coupled run from the private fixtures; `trig1-tap-release` through
  the socket gives PCM `883476213933333f0c5f4e2b343f65ed470fda0c50c8fa37905972457e819cdd` (cold-start reference state, 2026-10-07; was `aa1dbd9e..`), the same as `coupled-run`.
- No GUI tests; the desktop app shares the controller.

## Phases

| Phase | Size | Work | Unlocks |
|---|---|---|---|
| P0 | S | move `actor()`/`Command` into `digi-control::controller`; Tauri uses it; scenario exit becomes an event; no behaviour change | one owner for both hosts |
| P1 | M | proto, Unix server, discovery, `digi-emud`, run modes and jobs, status, step, run_until, inputs with `at_instr`, framebuffer, quit; `digi_client.py`; `emu:serve`; pytest | scripted CF tests without a GUI |
| P2 | M | captures as methods, `mem.cf/dsp.read`, event stream, `state.save/load`, scenario and replay over the socket | audio diagnostics and soaks from Python |
| P3 | M–L | `new_patched`, snapshot rules, cold-boot fallback and patched-snapshot cache, DSP-patch interpreter fallback, fast tables keyed by DSP image | quick patched-firmware tests |
| P4 | L | browser worker on the same method set; publishing work below | one client API for every host |

## Publishing (GitHub Pages)

- The public build ships only firmware-free code: no .syx, sections,
  snapshots, DSP image or state, generated core, or coupled wasm built with it.
  `astro build` fails if a firmware-derived file reaches the output (build
  guard in `apps/web/astro.config.mjs`); the coupled core lives in the
  gitignored `apps/web/.local/` and is served only by the dev server.
- The user supplies their own .syx at run time; it stays in the browser.
- The DSP image must be derived from the .syx inside the wasm (Rust port of
  the boot-stream parse and flatten done by `tools/sharc_dev/sharc_pack_image.py`; the
  ELZ decoder is already Rust).
- DSP state: the snapshot's DSP state came from a diagnostic chain and cannot
  be rebuilt. The browser needs (a) a modelled DSP boot (boot ROM/INIT handoff,
  clocks, DDR) reaching the ready state from the image, or (b) a one-time
  coupled cold boot in the browser cached in IndexedDB by .syx SHA-256. Both
  **[O]**; (b) depends on (a) or on attach-from-frame-0 working from a clean
  state, and would take minutes at current wasm speed.
- No generated AOT core (firmware-derived): the browser runs the interpreter
  plus a run-time wasm JIT (not built **[O]**). Without it, real-time audio in
  the browser is not expected.
- The protocol is transport-agnostic: the worker accepts `{id, method,
  params}` over `postMessage` and dispatches through the same `proto` types;
  `proto` and `controller` must build without threads or cpal (the
  `digi-coupled` feature split).

## Risks

- UI stalls: jobs and the UI pump share one thread (bounded to one chunk), but
  `diagnostics` and exact counters wait for the DSP pipeline.
- Frames stolen from the UI if a control path calls `snapshot()`; enforce
  `current_frame()` with a test.
- Patched loads that look fine but are wrong; fail loudly on each signature
  lookup.
- Stale AOT/fast tiers over patched DSP code give wrong audio silently; the
  interpreter fallback must be forced. Does the engine check the image against
  the gen? **[O]**
- Lost events: `lagged` must be treated as a failure by scripts.
- With `--control` on by default, any local process of the same user can drive
  the app (accepted: local only).

## Verification criteria

- P0: desktop `cli_tests` and fixture gates pass; `coupled_ready_exactness`
  hashes unchanged.
- P1: `step(N)` advances `icount` by exactly N; an `at_instr` input is recorded
  at exactly that value; replaying `inputs.recorded` gives the same state
  digest; chunk timing with no client matches the current build (A/B under
  `benchlock`).
- P2: socket render of `trig1-tap-release` matches the findings hash; a DSPI
  log started at frame K equals the env-var log from K on; `mem.dsp.read`
  agrees with `DIGI_DSP_WATCH` rows.
- P3: the stock .syx through `new_patched` behaves like `new`; a one-byte MAIN
  patch gives `start_used: cold` then a cache hit; a DSP-only patch gives
  `dsp_tier: interpreter`.

## Open questions for Em

1. Patched firmware: is an explicit `base_sha256` enough to skip the digest
   checks, or only in the daemon?
2. Patched MAIN: may `auto` cold-boot on its own, or require `start: "cold"`?
3. `--control` on by default in `mise run emu`?
4. DSP patches: interpreter-only for now, or regenerate the core per patch
   (~4 min + PGO) in P3?
5. Browser DSP state: modelled DSP boot or a one-time in-browser boot?
6. Should `coupled-run` become a `digi-emud` client later?
