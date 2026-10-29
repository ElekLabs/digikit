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
