# Private two-track binding proof

`sample-bindings.s` is an isolated assembled-consumer fixture, not an SDK ABI,
transport receiver, production pool, or hardware diagnostic. It includes the
existing SDK admission/epoch/count macros and actual PCM16 kernel; no numerical
or lifecycle primitive is reimplemented here.

Tracks 0 and 1 have independent controls (open, closed, count, fault, ready,
token), immutable source/frame descriptors and 128-frame packed PCM16 backing.
Other track values fail closed. Input routing uses generated `DK_IN_W_TRACK`.
Each private voice has six kernel cursor words, cached epoch +6, source +7 and
track +8. Cached ownership, epoch, admission and acquisition are checked under
one local interrupt-mask scope. Fresh note callbacks acquire before reading the
descriptor and release immediately afterward; render callbacks acquire before
using the cached pointer and release after the actual kernel's final read.
There is no note-lifetime pin or reliance on a note_off/voice-steal callback.

Private producer calls take track in R4, expected token in R8 and successor token
in R12 for publish. Result R0 is 0 complete, 2 pending, or -1 invalid. Close must
match track/token and closes admission before checking exact drain. Publish
requires closed=1, open=ready=0, zero count/fault, and a strictly increasing
positive signed 32-bit successor (initial token 0 may become 1). No retired token is
reopened and exhaustion cannot wrap. Faults are never cleared by the helpers.

One serialized producer fills exactly 64 words only after successful drain,
models completed payload delivery, then rechecks identity/drain and installs
source/frame/token before READY last. Its bounded fill is not a wait/spin loop;
there is no DMA or CF acknowledgment implementation. It cannot fill or publish
while a reader remains admitted. Real transport completion/ownership is a
separate prerequisite, absent from this diagnostic.

The Python test uses distinct A, B and producer stacks. It preserves A's paused
register/call/loop state while sharing DM, verifies A's live stack frame remains
unchanged across B's real kernel call, then checks preserved callee registers on
resume. This models scheduler interleaving, not physical interrupt entry/priority,
NMI exclusion, DMA completion, cache coherency, device backing ownership or timing.

Run only `uv run pytest -q tests/patchkit/test_dsp_sample_bindings.py`.
