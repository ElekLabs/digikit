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
