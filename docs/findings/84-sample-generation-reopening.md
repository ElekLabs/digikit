# Sample generation reopening: decisions and proof boundary

2026-10-29. Analysis targets: DT2 OS 1.17 and DN2 OS 1.12. **[D]** records a
source read or measured check; **[O]** marks a proposal or unresolved obligation.
No new [V], public sample ABI, hardware or performance claim. This follows the
[terminal retirement proof](83-sample-retirement-design.md); its existing evidence
remains there. Current work is tracked in the
[sampler plan](../plans/dsp-sdk-and-sampler.md).

## Implemented cached-voice guard

**[D]** The diagnostic consumer now reserves 28 bytes of state and clear size:
the first six words retain the SDK cursor contract, and private word 6 holds the
admitted bank generation. A note initializes that word to zero, then captures the
current nonzero token only with READY=1 inside the existing masked admission
section. Render compares the cached token with the current token in that same
section before incrementing the admitted count. It cannot adopt a newer token.
Denied callbacks invalidate the word and return silent DONE. Counter corruption
still fails closed; the numerical reader still runs with incoming MODE1 restored.

**[D]** Forty-two focused assembled diagnostic/CF tests pass, including six new
cases covering a changed global token, invalidated cached renders, fresh-note
capture, denied notes and armed-like cursor contents. The rebuilt DSP object
re-decodes 832 instructions and passes its freshness check. This exercises actual
consumer assembly with modeled control-word changes; the wire protocol remains
unchanged. It does not prove an actual bank reopening or a queued note's age.

**[D]** Direct Python lint/format, scoped TypeScript lint/format, regression types,
path and diff checks pass. Host checks pass 193 with 16 skipped
(`out/runs/ddr-proof/generation-host-checks.log`). Routed changed checks retain ten
known path-reference failures and the pnpm launcher failure
(`out/runs/ddr-proof/generation-changed-checks.log`); no clean integration gate is
claimed.

**[D]** The parent reports all five existing coupled cases pass against the changed
consumer/state size: full transport/ten-second playback, three fresh-boot failure
cases and terminal retirement during playback. Log:
`out/runs/ddr-proof/voice-generation-suite.log`; its 119.59-second wall time is not
a performance measurement. Normal artifact:
`out/runs/ddr-proof/55f9f2f3-ff27-47d5-aa3b-1d80f4054be6`; terminal artifact:
`out/runs/ddr-proof/d1974ff3-48fa-46f1-8bec-6feb6b6f9314`. This completes the
independent cached-voice guard check, not a generation-reopening wire test.

## Pending queued-note policy

**[O]** The user is deciding whether a note queued before selection but dispatched
after the new sample becomes READY should play the new sample or be discarded.
Preserving those future sequence events is the recommended option currently put
to the user. “Stop old notes on selection” settles currently playing notes; it does
not by itself classify every queued note as stale. Neither option is assumed here.

If dispatch may resolve the newly selected sample, the cached-voice guard can
protect backing pointers without claiming strict selection-time event rejection.
If old queued events must be discarded, generation capture before queueing or an
established upstream ordering fence is required. Capturing a token only when a
note reaches DSP cannot establish when the event was originally queued.

## Proposed next private protocol

**[O]** This is a design candidate, not implemented protocol. The next bank token
must be the exact boot-scoped successor `current + 1`, accepted only after the
prior generation is exactly RETIRED, admitted count is zero and fault is zero.
Tokens must never reset to zero, be reused or wrap; exhaustion fails closed.

| Operation | Sequence / status | Proposed behavior |
| --- | --- | --- |
| INIT / DATA | Existing transfer sequences / 0 | Load only the accepted successor generation while admission remains closed. |
| COMMIT | 376 / 1 | Validate the complete bank and seal it with admission closed. Status 1 must never open admission automatically. |
| OPEN, op 5 | 378 / 7 | Open the exact sealed generation only after the chosen queued-event policy fence. |
| RETIRE, op 4 | 377 / 5, or 6 while pending | Close and drain the exact generation; a complete sealed bank may retire without ever opening. |

Control sequences are exact operation identities, not a generic increment rule:
COMMIT → OPEN → RETIRE uses 376 → 378 → 377. CF must match the exact operation,
token, sequence and success status. A duplicate OPEN received after RETIRE must
never reopen that bank. Generic status-1 auto-opening must be removed entirely
when implementing this design.

**[O]** To skip selection B after starting its load, finish and seal B, retire B,
then begin C. Cancellation is not part of this candidate. One serialized writer
must own the whole transfer transaction. Any uncertain completion stops that
writer queue; it must not continue to C and risk overlapping backing mutation.
A local busy flag or serialization of individual packets is insufficient.

Required focused checks include stale BEGIN/INIT, DATA, COMMIT, OPEN, RETIRE and
ACK messages; exact successor rejection; duplicate OPEN after retirement; sealed
B retirement without opening; rapid A → B → C; uncertain-completion queue stop;
and token exhaustion. Actual close/drain/load/seal/open playback must then be
proved in a coupled run under the chosen queued-note policy.

## DN2 queue discovery relevant to that decision

**[D]** One-pass current DN2 1.12 decompiler/scout reads report the following. These
are discovery observations, not byte-audited [V] facts or approved interception
points. Reproduce reads through the existing `tools.coldfire.fw` decomp/dossier
workflow documented in [finding 83](83-sample-retirement-design.md).

- `ADM_TRIG_HW` value 17 maps to frame `0x80005e60 + 0x22 = 0x80005e82`;
  `0x80005e84` is not that field. Builder `0x40025e36` writes the high half of
  `local_b0`. Its generic event path resolves note/velocity through `0x4002a6a2`
  and ORs the allocated mask when payload byte `0xe` has its sign bit set.
  The exact event type and all producers remain unproved.
- Timeline insertion `0x40138e3c` handles heterogeneous timestamped nodes;
  the payload chain is at offset `+0x68`. No spare or unused payload bytes have
  been established for a generation field.
- `0x400d7d76` pops through `0x40138cb6`, sets type 7 and inserts an event.
  `0x400d7e88` copies sixteen `0x6c`-byte records from `0x446494c4` and inserts
  them with inherited type. `0x400255b4` copies a type-2 control record.

**[O]** Clearing this heterogeneous timeline is unsafe as a general note fence.
A quiet frame does not establish an event-age fence either. If the user chooses
strict discard, a centralized generation-capture candidate and coverage of all
producers remain to be established before implementation. Existing pending-frame,
DMA and callback assumptions in finding 83 still apply.
