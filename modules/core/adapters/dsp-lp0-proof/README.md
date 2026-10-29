# Private DN2 LP0 receive diagnostic

Explicit opt-in for the emulator, DN2 OS 1.12 only. Requires `core/expansion` and
conflicts with `core/dsp-link`, `core/runtime` and `core/dsp-ddr-proof`. It is absent
from device module sets. Never run this diagnostic on hardware.

The AudioTask common tail initializes LP0 once. Before any driver mutation it
requires live IMASKP, owner, LP0_CTL and DMA30_CFG all zero. The current task chain
and driver ABI are recorded in [finding 83](../../../../docs/findings/83-sample-retirement-design.md)
and [finding 92](../../../../docs/findings/92-dn2-native-bulk-seam.md). Calls use hash-bound
anchors and normal stock call frames. Private L2 allocations hold a 16-byte aligned
128-byte state, handle, seven-word descriptor and aligned 4,100-byte receive buffer.
The descriptor is reconstructed before each asynchronous submit.

The callback accepts event 4, its exact context, the exact normalized RX byte
address, and sequential tags 0 through 234. It copies only to the existing private
experimental bank `0x80800000..0x808ea600`, clamps the final page to 1,536 bytes,
and verifies both full 4 KiB canary bands before completion. No external range is
accepted. The bank is never published to voices. It neither writes reply/ACK rows
nor replaces command 4 or the normal frame sender.

DSP symbols `_lp_status`, `_lp_error`, `_lp_pages`, `_lp_bytes`, `_lp_rx` and
`_lp_handle` are diagnostic observations. Status is 0 before initialization, 1
initializing, 2 armed/loading, 3 complete, or 4 sticky error. Errors are context
1, occupied/active port 2, driver error 3, or callback/progress/canary error 4.
There is no teardown or reopen; errors require a fresh boot. A driver failure may
already have mutated its private ownership/queue state.

ColdFire `lp_mail` is four u32 words: state, error, tag and sent count.
`lp_staging` is 1,024 numeric words in big-endian CF memory. The host writes the
whole payload and tag, then publishes state 1 (START) last. The UI-task tick changes
state to 2 (BUSY), calls the boot-initialized mutex-protected stock sender with
format 1 (least byte first), and publishes state 3 (DONE) only after return. Invalid
or nonsequential requests produce sticky state 4 (ERROR). Staging and the request
remain retained until DONE; DONE does not imply DSP consumption.

The stock sender has an unbounded hardware-readiness wait. Before **every** START,
the emulator runner must observe an armed DSP DMA and the expected page progress,
retain one request until the sender returns, and impose an external bounded timeout.
These memory observations are not a production ACK protocol or a bounded CF API.
First prove one 4 KiB page, callback and rearm. Only after review proceed to all 235
packets / 960,000 bytes while checking normal audio frames. Both native emulator
proofs now pass; results and limits are in finding 92. The full-bank case requires
`DIGI_LP0_FULL_BANK=1` in addition to the explicit prebuilt daemon/patchkit paths.
Ordinary frame progress does not establish audible coexistence or pool ownership.

Focused checks: `uv run python -m pytest tests/patchkit/test_dsp_lp0_proof.py -q`.
Driver stand-ins test our call frames and guards; they do not prove the real DMA,
ISR, cache behavior, private memory availability or hardware safety. CF tests reuse
the existing `cf_call` executable and skip if it is unavailable.
