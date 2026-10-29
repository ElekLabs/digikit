# Per-track sample reader lifetime

2026-10-30. Isolated assembled SHARC consumer proof for the
[binding design](../design/sample-binding.md), using existing SDK primitives.
This does not change the public sample API or establish a production pool.

**[D]** `tests/fixtures/dsp/sample-bindings.s` routes generated input track 0 or 1
to independent controls and two 128-frame PCM16 banks. Private voice state holds
the SDK cursor, cached epoch, source pointer and track. Both tracks deliberately
start with equal epoch values: cached track identity, admission, epoch and reader
acquisition are checked together under a short local interrupt mask. Fresh notes
pin descriptor reads; render callbacks pin the actual SDK PCM16 kernel's reads.
Release follows the last possible backing access. No pin survives for a note's
whole lifetime, so an idle/stolen voice needs no note-off callback to drain.

The private producer matches track/token before closing admission. It cannot
reuse or publish until closed, not ready/open, exactly zero readers and no fault.
It models completed payload delivery with a bounded fill, rechecks identity and
drain, installs source/frame/token, then publishes READY last. Successor tokens
must be strictly increasing positive signed 32-bit values; exhaustion fails.
Corrupt release bookkeeping fails closed and poisons the count rather than
establishing a completed drain.

**[D]** All 15 cases in `tests/patchkit/test_dsp_sample_bindings.py` pass:

- Both tracks produce independently calculated, exact nonzero float PCM through
  the real SDK PCM16 reader, with no lingering callback pin.
- Pausing A inside the actual reader or immediately after its return prevents
  A's retirement/reuse. B still renders its exact prior PCM. A resumes, releases,
  drains, and accepts a replacement; its old cached voice then produces silence
  without a backing read, while a fresh note plays the replacement.
- A note arriving during closure cannot bind the old generation. Pausing a
  note before its descriptor reads also prevents reuse until that callback releases.
- Invalid tracks, mismatched cached ownership, stale commands, invalid successors,
  maximum-token exhaustion and a corrupted release cannot adopt another binding
  or reopen retired backing. B remains independently playable.
- At the before-READY breakpoint, payload, descriptor and token are complete
  while admission is closed. A fresh note is denied until final publication.

**[C]** Independent review caught shared stack tops in the first interleaving
harness: B's kernel could overwrite A's suspended frame despite retained registers.
The final harness uses separate A/B/producer stacks, snapshots A's populated
40-word stack window and checks it unchanged after B, then checks distinct saved
register values after resumption. A second review found no remaining issue.

Validation: 15 tests pass in the final focused run; scoped Ruff format/lint and
diff checks pass. Logs: `out/dsp-sample-bindings-{tests,format,lint,diffcheck}.log`.
No existing proof, shared SDK implementation, emulator core or generated core
changed. The fixture's [README](../../tests/fixtures/dsp/README.md) records its
private calling convention and limits.
Routed `test:changed` passes Ruff and layering; its only failure is the same ten
baseline stale references in five unchanged files (`out/binding-proof-changed.log`).
Scoped changed-document path checks pass. No full merge gate is claimed.

**[O]** Preserving paused register/call/loop state while sharing DM models a
scheduler interleaving, not physical interrupt dispatch, NMI exclusion or DMA.
The payload fill is not the production transport. Cache behavior, checked owned
backing, source validation, CF/DSP close/drain acknowledgement and runtime/picker
integration remain unproved by this fixture. No hardware or performance claim follows.

## Private compiled CF close/ACK boundary

**[D]** `tests/fixtures/dsp/sample-binding-close.s` includes the unchanged
assembled reader and wraps `_bind_close` with a private serialized logical
CLOSE/ACK protocol. `sample-binding-close.c` runs as actual compiled ColdFire code
through the existing prebuilt call harness. The test driver delivers complete
immutable four-word numeric snapshots; this is not physical wire serialization,
concurrent publication or firmware transport. READY-last does not establish
atomic multiword snapshots.

Request header `0x01014243` precedes positive signed request ID, track 0/1 and
positive signed expected epoch. Reply header `0x00014241 | status << 24`
precedes the same tuple; statuses are PENDING=1, DRAINED=2 and REJECTED=3.
DSP checks header, identity and serialized request history before closing.
Identical retries recheck drain; lower/conflicting IDs and a higher ID while
pending reject without replacing history. A new accepted ID clears the previous
drained grant. Replayed close after replacement cannot drain the new epoch.

CF keeps its request immutable while pending and authorizes reuse only on an
exact-tuple DRAINED reply. Unrelated ACKs are ignored. Matching malformed,
unknown-status and REJECTED replies fail closed; timeout is sticky quarantine,
and late replies cannot change terminal DRAINED/FAILED. Neither processor frees
buffers or exports a new public ABI here. See the
[fixture contract](../../tests/fixtures/dsp/README.md#private-cf-closeack-boundary).

**[O]** This narrows the earlier CF/DSP close/drain acknowledgement gap only for
complete logical snapshots. Actual transport ownership/serialization, physical
IRQ/nesting, cache-safe RX and production backing still need integration. No
native coupled run, hardware or new production protocol is claimed.

**[D]** All 22 focused cases in
`tests/patchkit/test_dsp_sample_binding_close.py` pass. The main interleaving
pauses A inside the actual PCM kernel, observes DSP/CF PENDING, renders exact
old B PCM with A's populated stack unchanged, resumes A/releases, then retries
the identical CLOSE to obtain DRAINED before publishing replacement A. Cached
old A reads no replacement backing; fresh A and unaffected B render exact PCM.
A separate paused-B case checks that a higher pending request excludes a third
request and preserves both bank controls and accepted history. Tests also cover
wrong track at equal epoch, wrong ID/epoch, malformed/unknown replies, stale and
conflicting commands, faulted readers, sticky timeout/late ACKs and ID exhaustion.
The shim checks distinctive R3/R5/R6/R7/R9, I4/I12/I13 and exact I6/I7/MODE1;
CF calls check the compiler's callee-saved convention. The existing 15-case
fixture/helper file was not changed. No emulator-core build was performed.

Independent review found and resolved a machine-specific runner path; the suite
now defaults to the repository's prebuilt runner with an explicit optional override.
Follow-up review found no remaining issue. Scoped path and whitespace checks pass.
Routed `test:changed` passes Ruff and layering and retains only the ten baseline
stale references in five unchanged files (`out/sample-binding-close-changed.log`).
No full merge gate is claimed.


## Private LP0 0.3 transport/control integration

**[D]** The private `core/dsp-lp0-proof` 0.3 diagnostic now receives logical
CLOSE `{0x01014243,id,track,epoch}` through its LP0 callback and multiplexes exact
CLOSE replies into the existing 16-byte reply extension. CF keeps distinct
seqlock progress/control snapshots, serializes both request paths and requires
an exact DRAINED tuple before DONE. Bounded pending rechecks run at the saved
AudioTask tail without resend; malformed matching replies, rejected requests,
65,536 UI polls or global driver error quarantine requests without releasing
buffers. Driver arm failure overrides DRAINED. Version 0.3 intentionally rearms
after the full payload, superseding 0.2 terminal-inactive behavior.

**[O]** These are two diagnostic control records, not actual audio-reader
bindings. The 22-case logical proof retains its separate actual-reader evidence.
Production backing/reservation, physical cache coherence, source validation,
real voice integration and picker remain open. The opt-in CLOSE case requires
an exact 0.3 patched-image hash.
See the [diagnostic contract](../../modules/core/adapters/dsp-lp0-proof/README.md).

**[D]** All 73 focused LP0 owner cases pass, retaining the earlier guards and
adding reply multiplexing, compiled CF/assembled DSP exact tuples, pending poll,
serialization, sticky malformed/rejected observations, timeout and arm-error
priority. ISR-visible identity is immutable/volatile and published active last;
matching bad evidence is latched before later frames can replace it. DRAINED
retires the active tuple as terminal claim before checking that evidence and
publishing DONE. Late tuple replies cannot change the terminal outcome.
Module-owned CF/DSP objects built/redecoded with existing patchkit under the
shared lock (450/727 instructions); scoped Python/TypeScript static checks,
generation/current-object, paths and diff checks pass. No emulator/core build
or hardware proof was performed.
The callback mask spans stock rearm; tests preserve incoming scalar-call MODE1,
bank selectors, callee state and stack, without proving physical IRQ/cache behavior.

**[D] Parent checkpoint:** independent code review caught a matching bad ACK
being overwritten before UI polling. The ISR sticky latch and terminal-claim
ordering above fix it; follow-up review reports no remaining issue. The routed
changed-files check passes Python/layer checks and retains the ten stale references
in five unchanged files plus the linked pnpm launcher failure. Scoped direct
checks pass; no merge gate is claimed. Log:
`out/runs/lp0-close-fixture/changed-check.log`.

**[D] Fresh boot fixture:** `out/runs/lp0-close-fixture/provenance.json` binds
patched SYX SHA-256
`6ff3b5ec9d9ed80f1108746922a213a75c9827a867fb257c1690c419fa56a2c8`.
The existing cold-start executable reached idle PC `0xb88a5d` after 563,750,000
instructions, within the 800M/120-second bounds. Parent inspection of exported
memory reads all four concrete XCNT fields as `0x566` before CF exchange.
The private packed image is a physical stock copy, and unchanged ready inputs
remain linked to their original files. No live descriptor resizing or core
regeneration occurred. Native CLOSE acceptance did not launch: its exclusive-lock acquisition expired
after 600 seconds behind another session's shared `verify runtime-fixture` holder
(observed PID 56574). No native CLOSE pass is claimed at this checkpoint.

**[D] Independent fixture audit:** input/output hashes, cold-state entry/empty
overlay, idle marker/PC and all four concrete descriptor words agree. The retained
`SOURCE.json` has a stale `image: dn2-1.11` label; independently repacking the
actual DN2 1.12 SYX section 7 reproduces the private packed image byte-for-byte
(6,424,692 bytes, SHA-256
`fea39d589ac45b20a6dfdb3f6c89a5b9d28fd7936ff819a7a09f17f3ff847a2a`).
The label is not used as version evidence; original inputs remain unchanged.

The independent audit also relinked the current tracked CF/DSP objects in memory:
both payloads match the plan and patched section bytes, and all 117 defined symbols
match. The full cold-idle file SHA-256 is
`1e9ffc2d16406889d845d04fa3d9856362d33badb7b1ca2c72f798b7dae4ad22`;
the log's `state_sha256` names the serialized payload rather than the full file.
The parent used `subprocess.run(..., timeout=120)` for cold preparation. Artifacts
record the deadline and actual 44-second completion, but do not independently
expose that external enforcement wrapper. Native resume command/environment and
lock outcome are retained as `native-command.json` and `native-lock-result.json`
in the same private fixture directory. The lock waiter exited; it started no
native daemon and did not stop or modify the other session.
