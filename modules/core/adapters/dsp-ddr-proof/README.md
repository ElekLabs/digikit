# Isolated DN2 DDR transport diagnostic

Experimental, DN2 OS 1.12 only. Patch explicitly with
`apply --modules core/dsp-ddr-proof`; it conflicts with `core/dsp-link` and is
absent from default patch sets. The acceptance test loads that prepatched image
through the existing stock session's `firmware.load` API, using prebuilt binaries.
It uses the existing CF frame-call and DSP common-tail hooks.

The host writes a dedicated 2,688-byte `dd_frame` staging array, then publishes
`dd_mail.state = 4` last. The four CF mailbox words are state, error/status,
watchdog ticks and frames sent. States are IDLE=0, BUSY=1, DONE=2, ERROR=3,
START=4. Do not touch staging while BUSY. Errors are sticky until boot. Staging
remains allocated after errors. This is a host test aid, without a runtime API.

The CF wrapper passes the original arguments to the stock sender while idle.
Substitution requires stock command 3, lengths 0xa80/0xabc and zero masks at
source halfwords 17..20 in both this and the prior ISR sample. Otherwise it
passes the normal frame and waits at most 900 ISR ticks. This check does not
establish a fence for queued events or authorize loading during live audio.
The stock sender synchronously builds its SPI queue from the staged halfwords.

Each DSP word is transmitted low halfword first. CF halfwords are stored in the
CF CPU's big-endian order. The 672-word full frame is:

| Word | Meaning |
| --- | --- |
| 0 | Exact command 4; the stock command rejection path mutes output and reaches the common tail |
| 1 | Distinct proof magic `0xDDF4A601` |
| 2 | Nonzero exact 32-bit token |
| 3 | Sequence: INIT 0, DATA 1..375, COMMIT 376 |
| 4 | Operation: INIT 1, DATA 2, COMMIT 3 |
| 5 | Destination offset in words |
| 6 | Payload count, at most 640 words |
| 7 | Checksum over words 0..6 followed by payload: `s1 += w; s2 += s1; check = s1 XOR s2`, modulo 32 bits |
| 8..31 | Reserved zero, excluded from checksum |
| 32..671 | Up to 640 payload words |

INIT adopts a token once per boot, clears READY, and initializes 4 KiB canary
bands at byte ranges `[0x807ff000,0x80800000)` and
`[0x808ea600,0x808eb600)` to word `0xA55A3CC3`. DATA must match the next sequence
and exact next offset. All range and checksum checks precede copying. The
immediately preceding identical accepted request is acknowledged without
rewriting. Another token is ignored; owned malformed frames poison the proof,
including before INIT. COMMIT requires exactly 240,000 accepted words before
publishing READY last. The destination normal-word base is `0x10200000`, the
emulator's byte range `[0x80800000,0x808ea600)` (960,000 bytes).

The acknowledgement occupies exactly record bytes `0x60..0x67`: word 0 has
magic `0xA6DD` in bits 0..15, sequence in 16..24 and status in 25..31; word 1
contains the exact token. Status 0 means accepted, 1 READY, 2 checksum error,
3 range error, 4 protocol error. CF-only errors are 100 timeout and 101 malformed
staging header. This adapter owns that region exclusively through its conflict
with the normal link adapter.

Focused tests: `tests/patchkit/test_dsp_ddr_proof.py`. The coupled acceptance test
is `tests/regression/runtime/sample-ddr-transfer.test.ts`; run only after review,
with an explicitly supplied prebuilt DN2 daemon. It captures the baseline,
checks the entire payload and both complete canary bands through bounded reads,
and uses fresh boots for sticky failure cases. No hardware cache/coexistence,
product sampler, resource lifetime or live-audio loading result follows from it.


## Fixed resident playback stage

The same isolated adapter declares the patched-in `DDR proof` machine and its
`DDRPROOF` page, with a Reverse knob (0..1, default forward). Its diagnostic-only
permanent DN2 type is reserved in `core/machines/types.toml`; defaults still do
not include the adapter. The closure adds `core/machines`, `core/params` and
`core/dsp-voice`, without the conflicting DSP-link adapter.

Note-on admits only READY=1, latches a six-word cursor covering 480,000 mono
frames at 48 kHz and uses the packed PCM16 SDK reader. The source address is
fixed inside this core diagnostic, remains immutable through reboot, and has no
public resource ABI or replacement path. Reverse starts at 479999 with rate -1;
forward starts at zero with rate +1. Unready notes are silent and DONE. The
bridge preserves the adapter's I12/I13 and exact stack boundary across DK_CALL.
The descriptor uses 24 bytes of state and conservative instruction budgets
2400/2600 (render/note-on including render), without a hardware cycle claim.

The consumer unit test is `tests/patchkit/test_dsp_ddr_playback.py`, including
forward/reverse retriggers, end-of-view completion and an extra silent render
with no source reads after DONE. The coupled valid scenario verifies the entire
transport and canaries before selecting this machine, then records a complete
WAV, live registry binding, early/late/end state and known 200 Hz content. Its
normal-command counter observes low-halfword command 3 in actual receive frames
after COMMIT; its 32-bit load includes adjacent pitch, which the observer masks.
The transport gate still requires exact complete word 4 plus its distinct magic. A stock
engine workspace peek is diagnostic only; downstream reuse and FX tails do not
prove the kernel's after-DONE output.


The completed DN2 emulator run is retained at
`out/runs/ddr-proof/11723539-7f66-4fce-b663-45cd9ea3d3da` (log
`out/runs/ddr-proof/transport-playback-fixed.log`). Full payload and canaries pass,
actual type 11 dispatches one note, and 480,000 frames complete in 15,000 renders
with DONE. The 552,992-frame stereo capture is finite and preserves the known
200 Hz source in both early and late windows. Normal frame observations advance
after COMMIT and through playback. Eighteen focused checks pass. This proves the
isolated emulator capacity/playback milestone; hardware, replacement lifetime,
Transfer UI and production resource APIs remain separate work.
