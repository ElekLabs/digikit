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
| `cf.expansion` 0x47e8b000..0x47fe0000 | above the image end 0x40348b90 and the bss end 0x45566490; the record (64 bytes) is at its start. **This region is inside the 4 MiB delay ring 0x47c00000..0x48000000 and is not usable at run time; the corrected region is in [finding 138](138-syntakt-runtime-facts.md).** The byte check of the dry run is unchanged. |
| Checksums | 19,811 packet checksums and counters, framing counts, content checksum 0x65ca2f7e over 2,000,880 bytes: both readers agree. There is no trailer slot (finding 70) |
| Unchanged rebuild | `patchkit rebuild` of the stock file gives the stock file byte for byte **[V]** |

Instruction operands of the stock MAIN OS that name the cave (2) or the expansion region (10) are listed in the report. All are
data decoded as code or constants: for example `move.l #$47faccf0,d3` at 0x40095818 is an EMAC coefficient (function 0x4009580c).
DT2 1.17 has 2 such operands, Syntakt 10. A semantic check of each is **[O]**.

## Runtime target

core/runtime with core/expansion patches 1.42 and boots in the emulator ([finding 138](138-syntakt-runtime-facts.md) for the device facts).
What made it possible, all in the tree:

- The runtime has no device value in its code: the facts that differ between devices are the fields of the device object `dkrt_dev`
  (`struct DkrtDevice` of `modules/core/runtime/abi.toml`, written per device by the `gas-dkdv` gen kind). `[capabilities]` and the tables
  present give its capability bits; DSP facts are optional in `[board]` and `[digikit]` for `dsp = "none"`, `[card.store]` and
  `[board] esdhc_base` and `[digikit] safe_key` are optional too (the runtime then has no store, no card polling, no safe key).
- A manifest's `optional_anchors` names anchors and hooks that a port may lack: the link is 0, a hook op is skipped, the plan lists
  `disabled`. The runtime lists `cf.link.site.block_isr.counter_lit`, `cf.samples.fn.worker.get` and `.queue`, and the two page hooks
  (no site on 1.42) and their sites; the code checks each link for 0.
- The hooks `cf.midi.hook.rx`, `cf.rtos.hook.main_os_task` and `cf.view.hook.ui_loop` name no devices (every device whose site resolves has them).

Values of `devices/syntakt.toml [digikit]` and `[board]` are in finding 138. The safe key is the firmware key index 39 (trig 16), because the runtime
asks `key_down(k)` with a firmware index.

### Dry run, core/runtime + core/expansion on 1.42 **[D]**

`tools/patchkit/image_report.py` over the stock file, the patched file and the plan: ALL CHECKS PASSED. Only section 3 differs; sections 1, 2, 4, 5, 6, 7, 8
are identical to stock. MAIN grows from 3,442,576 to 3,713,176 bytes.

| Write | Site | Size |
| --- | --- | --- |
| reset init call (core/expansion) | 0x4000053e | 8 |
| UI loop retarget | 0x4000b2d8 | 6 |
| main-OS task hook | 0x4000abd6 | 8 |
| MIDI rx hook | 0x40103f5e | 6 |
| loader in `cf.flash_cave` | 0x40339730 | 54 |
| runtime object in `cf.expansion` | 0x478c0000 | 980,416 |
| trampoline object, device object (with `dkrt_dev`) | 0x479af5c0, 0x479af720 | 352, 160 |
| hook tails (main-OS task, MIDI), patch record | 0x479af7c0.. | 16, 12, 80 |

Disabled (no anchor or hook on 1.42): `cf.link.site.block_isr.counter_lit`, `cf.samples.fn.worker.get`, `cf.samples.fn.worker.queue`, `cf.page.hook.source_key`,
`cf.page.hook.source_turn`, `cf.page.fn.source_key`, `cf.page.fn.source_turn`. The runtime uses 981,036 of the 3,342,336 bytes of `cf.expansion`;
the keep area (0x47bf0000) lies above its end.

### Boot in the emulator **[D]**

The patched file boots in the ColdFire-only emulator to the main screen (700 M ticks, `main_ui_reached`, `ready`): `DKRT_BOOT` has magic "DKBT", version 1,
safe 0, reason 0, `ui_reached` 1; `dkrt_dev` reads back caps 0xc (eSDHC, safe key), safe key 0x27, keep area 0x47bf0000, no DSP facts. The runtime's
event ring holds BOOT_START (runtime 0.7.0, device 3), RAM_KEEP (not kept: first boot), BOOT_RECORD and UI_FIRST. Over the emulated USB MIDI `dkupload --emu info` answers
(port `st`, runtime 0.7.0, capacity store 0/0 KiB, fx_slots 0/0) and `modules` lists no module and no DSP area. **Not done:** the debug module is a run-time module loaded from the module
store, and the Syntakt has no `[card.store]` (`DEV_HAS_STORE` clear), so it cannot load; its event log is the runtime's own ring, read from memory. A store needs the card layout of finding 138
and an eMMC image the emulator seeds (`capabilities.card` is `unknown`).

Missing anchors and hooks the other adapters need: core/ui (`cf.menu.site.settings_list.system_icon_ref`, `cf.menu.hook.settings_list.tail`), core/health (the DSP block interrupt
`cf.link.site.reply_copy_lit`, `cf.link.hook.block_isr`, and `cf.rtos.hook.idle`), core/input (a per-device `input_cfg` object and `[input]` for the Syntakt).

## Adapters on 1.42 (core/ui, core/health, core/input, core/digikit-menu, core/introblank) **[D]**

Anchors and hooks added (rule.st or `devices` list, all from the same code as on DT2: instruction by instruction at the site): the SETTINGS list rebuild
(`cf.menu.site.settings_list.tail` and its hook, `system_icon_ref`; the rebuild is 0x4005ed72, its last instructions equal DT2's, the hook site is 0x4005f176),
the idle site hook (`cf.rtos.hook.idle`, boot thread 0x40082578, site 0x40082896), the intro setup hook (`cf.rtos.hook.intro_setup.table_arg`, 0x40087d0a, same
function bytes). New site `cf.menu.site.vertical_menu_view_ctor.font_ref`: the VerticalMenuView constructor 0x400fed6c does the same steps as DT2's but the
class has three words fewer, so the font pointer is at +0x9c (DT2 +0xa8); core/ui now reads the font from the constructor's `move.l #font,d0` operand, on all devices.
`[input] backend = "panel-uart"` in `devices/syntakt.toml` and `modules/core/adapters/input/src/cf/st/device.s` (the `[panel]` map is the measured one).
Optional (no DSP): `cf.link.hook.block_isr`, `cf.link.fn.block_isr`, `cf.link.site.reply_copy_lit` (core/health): no window closes, the idle count and the heap walk work.

Dry run (`core/runtime`, `core/health`, `core/input`, `core/introblank`, `core/digikit-menu`, with `core/ui`, `core/expansion`): `image_report` ALL CHECKS PASSED, only section 3
differs, read-only sections identical, 11 differing ranges: reset call 0x4000053e, main-OS task 0x4000abd6, UI loop 0x4000b2d8, SETTINGS tail 0x4005f176,
idle 0x40082896, intro setup 0x40087d0a, MIDI rx 0x40103f5e, loader in the cave, the segment chain.

Boot (ColdFire-only `digi-emud`, 700 M instructions to the main screen): SETTINGS lists ..., SYSTEM, DIGIKIT; DIGIKIT opens its menu (ABOUT, HEALTH, MEMORY, MODULES) and the ABOUT
page shows the runtime version and free RAM in the firmware font. `health_idle` counts (the boot thread called the create-main function first); `health_hs` stays 0. A call of
`dk_input_key(5, down)` from a stub (core/input) sets bit 4 of the firmware's previous-mask byte 0 and the main screen shows TRK 5. The OLED lags the key by one UI pass in `framebuffer.get`
(a later key shows the earlier one's result). **Not checked:** the HEALTH and MEMORY pages (a second key after the menu opened did not change the screen; the cause is open), `dk_input_turn`.
