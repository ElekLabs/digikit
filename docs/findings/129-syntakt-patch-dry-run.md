# Syntakt OS 1.42: static patch dry run

All claims are **[D]** (read once) unless marked. Nothing was run on a device or in the emulator. Plan:
[syntakt](../plans/syntakt.md) P3. Container facts: [finding 70](70-syntakt-1-42.md).

## What was patched

`patchkit apply --firmware firmware/Syntakt_OS1.42.syx --modules core/expansion` writes
`out/syntakt/patched/Syntakt_OS1.42-expansion.syx` (plan: `plan.json`, report: `report.txt`, same folder). The report is
`uv run python -m tools.patchkit.image_report STOCK PATCHED PLAN`, which decodes both files without patchkit's reader.

| Check | Result |
| --- | --- |
| Sections 1, 2, 4, 5, 6, 7, 8 (stored and decoded sha256) | identical to stock |
| Section 3 (MAIN OS) | 3,442,576 -> 3,442,656 bytes: the 8-byte reset hook at 0x4000053e, the loader (54 bytes) in the cave, and the 80-byte segment chain after the image end |
| Hook site | stock bytes `4eba ff1c 4eba ff6e` equal the plan's and the port anchor `cf.rtos.fn.reset.init_calls` hash |
| `cf.flash_cave` 0x40339730..0x4033a000 | zero in stock, ends at the reset clear start 0x4033a000; the loader uses 56 of 2256 bytes |
| `cf.expansion` 0x47e8b000..0x47fe0000 | above the image end 0x40348b90 and the bss end 0x45566490; the record (64 bytes) is at its start. **This region is inside the 4 MiB delay ring 0x47c00000..0x48000000 and is not usable at run time; the corrected region is in [finding 130](130-syntakt-runtime-facts.md).** The byte check of the dry run is unchanged. |
| Checksums | 19,811 packet checksums and counters, framing counts, content checksum 0x65ca2f7e over 2,000,880 bytes: both readers agree. There is no trailer slot (finding 70) |
| Unchanged rebuild | `patchkit rebuild` of the stock file gives the stock file byte for byte **[V]** |

Instruction operands of the stock MAIN OS that name the cave (2) or the expansion region (10) are listed in the report. All are
data decoded as code or constants: for example `move.l #$47faccf0,d3` at 0x40095818 is an EMAC coefficient (function 0x4009580c).
DT2 1.17 has 2 such operands, Syntakt 10. A semantic check of each is **[O]**.

## Runtime target [O]

core/runtime cannot be applied to 1.42 yet. Nothing was changed for it.

- `patchkit gen` reads every device file with `[digikit]` as a runtime target and requires `[board] dsp_clock_hz`,
  `dsp_blocks_per_second` and `[digikit] dsp_area_bytes`, `dsp_tail_off`, `dsp_tail_words` (`devgen.rs` `Board`, `Digikit`;
  the error is `missing field dsp_clock_hz`). `devdata.rs` `validate` forbids these keys when `[capabilities] dsp = "none"`.
- The runtime's shared constants (`[[gen]]` rust and gas-shared in `modules/core/runtime/module.toml`), with the DT2 = DN2 value
  and what Syntakt has: `DSP_BLOCK_CYCLES` 666,667 (no DSP: no value), `SAFE_KEY_CODE` 19 (the RECORD code of the Syntakt panel
  is not measured), `DYN_TYPE_FIRST/LAST/DYN_TYPES` 16 / 23 / 8 (`types.toml` has no `st` table; no run-time machines),
  `ESDHC_BASE` 0xFC0CC000 and `CONSOLE_UART_BASE` 0xEC070000 (board not read), `DDR_*`, `KEEP_*`, `LIMIT_*` (could be equal).
  Per-device values (`device-<dev>.inc`): `STORE_FIRST`, `STORE_SECTORS` (`[capabilities] card = "unknown"`: no card layout).
- `patchkit check --modules core/runtime` on st lists: anchors `cf.link.site.block_isr.counter_lit`, `cf.samples.fn.worker.get`,
  `cf.samples.fn.worker.queue`; hooks `cf.midi.hook.rx`, `cf.rtos.hook.main_os_task`, `cf.view.hook.ui_loop` (their sites resolve on
  1.42, their `devices` lists name dt2 and dn2 only) and `cf.page.hook.source_key`, `source_turn` (no site on 1.42).
  `compatible()` refuses a module for a missing anchor; a symbol in the manifest's `optional` list is linked to 0 only when
  it is not an anchor name. So a missing anchor is a refusal today, not a "unsupported" answer at run time.
- `dk_time_blocks` already answers 0 when the counter anchor reads 0 (`dkrt_blocks`); `dk_debug_track_type` and
  `dk_debug_selected_params` call anchors (`cf.project.*`, `cf.page.fn.native_track_sound`) that resolve on 1.42 with medium
  confidence; their Sound layout is unchecked.
