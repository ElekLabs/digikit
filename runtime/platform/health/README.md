# core/health

Measures what the DIGIKIT HEALTH page shows: CPU load, the DSP's block time and the firmware heap. Emulator only: nothing here has run
on a device.

- **CPU.** The boot thread ends in an empty loop once every task exists (finding 36), so it runs only when no task is ready. A hook
  replaces the thread's last call, makes it and counts in that loop. The DSP block interrupt (1500 per second) closes a one-second
  window and keeps the iterations; load = 100 - 100 * window / best window since boot.
- **DSP.** The DSP's block handler reads EMUCLK at its start and end and puts the difference in the first word of its reply; a hook at
  the entry of the block interrupt handler reads that word once per block (window average and maximum). No DSP code is patched.
- **RAM.** The heap's free lists are walked on request, under the allocator's mutex, at most 4096 nodes.

The runtime's `dkrt_health` (C ABI) combines these with its own arena and DSP area; the DIGIKIT menu shows them. Results and costs:
[docs/findings/41-digikit-health.md](../../../docs/findings/41-digikit-health.md). Proof:
`uv run python -m tools.coldfire.digikit_health_proof run`.

Without a DSP (no block interrupt, `cf.link.hook.block_isr` and `cf.link.site.reply_copy_lit` are `optional_anchors`) the block
hook is not installed: no window closes, `blocks`, `windows` and the DSP figures stay 0, and the idle counter and the heap walk still work.

## CPU REL without a DSP (open)

The load windows are closed by the DSP block interrupt hook (1500 blocks, one second). A device without a DSP has no such hook, so `CPU REL` stays `--` (the Syntakt). The design, not built:
a window clock chosen by capability. The device file has `[digikit] span_clock` and `span_clock_hz` (the Syntakt: DTCN2, 132 MHz); the runtime (which has the profile) would hand `now` to
a new `health_tick(now, hz)` of this module before it reads `health_read`, and the module would close a window when `hz` ticks have passed. The HEALTH page asks about twice a second,
and only while it is open, so a window is not exactly a second: the idle count must be normalised to a rate (iterations per second of the clock) before it is compared with the best
window. That changes the `HealthRaw` layout and the runtime-to-module interface; it needs no new firmware hook. Estimated at more than the hour that this was given.
