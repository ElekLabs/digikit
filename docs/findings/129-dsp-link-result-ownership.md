# DSP link results have no owner

Static trace of `write_word_blocking` (`modules/core/runtime/src/rs/src/dsp.rs`) and the link state in `modules/core/adapters/dsp-link/src/dsp-link.c`. DT2 OS 1.17 and DN2 OS 1.12. Nothing was run, so the mark is [D]. It settles the open question in [runtime-and-services.md](../plans/runtime-and-services.md) section 6.

## Result

**[D]** `write_word_blocking` is not the cause by itself. It spins until `dl_send_win` accepts a send (`dsp.rs:1124-1132`; `dsp-link.c:158` refuses while the link is BUSY, START or ending). A stuck link ends the spin after `SPIN` = 4M tries with `E_BUSY` and `PH_RETAINED`, which is safe.

**[D]** The real hazard is that **link results have no owner**. The link keeps one shared state (`dl_poll`, `dsp-link.c:174-188`), and a new send resets `state` and `err` (`dsp-link.c:163-170`). A writer that has finished but has not polled yet would read the result of the next writer.

Sequence:

1. Writer A sends a word and the transfer ends.
2. Before A polls, writer B sends. The send resets the state.
3. A polls and reads B's result.

Writers that poll: `fxpage.rs:522-536`, `fxsend.rs:384-396`, `tracks.rs:166-180` and the loader step `PH_ROWS` (`call_send`, `dsp.rs:1315`).

Effects, if it happened:

- A false "done" in `fxpage` marks `s.dsp[k][j]` as sent (`fxpage.rs:528`). A failed knob write is then never sent again.
- A false "done" in `tracks` skips an operation step (`tracks.rs:174-176`) and leaves a stale track record.
- A false "error" only causes a retry, which is harmless.

## Reach

**[D]** The hazard is latent: it cannot happen in today's firmware.

- `fxpage`, `fxsend` and `tracks` check each other's `busy()` and poll before another one sends, so these three cannot overlap.
- Only the loader (`PH_SEND` rows, `call_send`, `dsp.rs:1315`) and `write_word_blocking` (`quiesce`, `clear_rows` and `clear_fx_rows` after a failed load, `dsp.rs:1156,1182-1194`) can send while another writer's result is pending.
- Loads happen only at boot, before `fx_live`, when no live writer is active.

It becomes reachable once any live load or unload path exists. Related: [99-stereo-frame-fetch-exclusion.md](99-stereo-frame-fetch-exclusion.md) lines 188-209.

## Fix

- Proper: owner-tagged results, the `submit(owner, ...)` FIFO of the DSP transfer queue (roadmap R6). Each request keeps its own result until its owner takes it.
- Minimum: the blocking path waits until the `fxpage`, `fxsend` and `tracks` busy flags are clear.
