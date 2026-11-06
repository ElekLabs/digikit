# Handover 2026-11-07: host-only asset prototype

User-authorized host-only follow-up on `work/loadable-bootscreens`, based on
`9e7c39db`. No firmware/runtime/SDK/module/Workbench, emulator/hardware, dependency
or environment changes. The provisional format remains experimental.

`tools.bootscreen.asset` packs exact PNG manifest inputs into bounded provisional
DKBS v1, inspects with a checked decoder, and previews decoded native frames.
Reuse: installed Pillow conversion/compositing, standard struct/zlib/base64,
existing oled_png with explicit different-layout conversion. No new encoder
framework, asset/provider loader or transfer path. See
[format/design](../design/bootscreen-assets.md).

49 named host tests, scoped Ruff and mypy passed. The decoder accepts valid
duplicate-frame records and XOR literal payloads larger than RAW, bounding decoded
output instead of requiring encoder choices. Unsupported Pillow modes (including
16-bit grayscale) fail with an 8-bit export message; Pillow may expose higher-bit-depth
RGB as RGB. Synthetic 180-tick moving-dot
asset is 5,715 B (180 records), constant dot is 42 B (one merged record); both
represent 184,320 raw tick bytes. Inputs/assets/previews are ignored under
`out/runs/bootscreen-assets-host/`. No stock artwork or performance claim.
Source was frozen after root/reviewer source review. Cheap preflight passed all
13 tasks in 1 min 2 s (8 cached); diff checks passed. Generated preview script
passes Node syntax checking. Interactive preview was not checked: the host has
no available browser. No browser dependency or mock-DOM harness was added.

64 KiB is host policy, not DKM/device staging proof. Runtime/render/storage remain
unsolved. Future scene_setup entry argument substitution needs ABI/byte/display
proof; indirect task JSR A2 retarget is not currently supported. Valid selection
is already owner evidence by current card.rs definition. Deleting redundant
marker reads using both successful selection reads plus capacity probe is only
a future design option, requiring explicit error tracking and PREP/card checked
reset on denial; it neither bounds latency nor authorizes a firmware change.
Blocked SDHC/DMA, read-only recovery and physical initial-scan/bypass limits remain.
