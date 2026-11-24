# Device data: where each device, board and firmware fact lives

Status: implemented in Phase 8 (lane b). Rule: a fact about a device, its board or its firmware that the digikit runtime, the core
modules, patchkit or `port_resolve` use comes from a data file, and code that needs a constant gets it generated
(`patchkit gen`, kept current by `patchkit gen --check` and the Rust test `packages/patchkit/tests/devdata.rs`). Code must not list
devices by name: `tools/nx/device-literals.mjs` (the Nx target `workspace:device-literals`; `lint` depends on it, and any change of
a tracked file selects it in the affected gate) counts device ids and names outside the allowed places and fails when the count in
any file differs from `tools/nx/device-literals.baseline.json`. The baseline holds the sites not yet moved to the registry; it
only shrinks (`node tools/nx/device-literals.mjs --write` after a removal).

What counts as a literal: a device name word (`digitakt`, `syntakt`, bounded by non-alphanumerics) and an id of three or more
characters (`dt2`) anywhere. An id of two characters (`st`) is an ordinary word, so it counts only at the start of a string token:
as the whole string (`"st"`) or followed by `-`, `_` or `${` (`"st-1.42"`, `` `st-${v}` ``). Not counted: `devices/` and `docs/`, the generated files under `ports/bindings/` (`patchkit gen` writes them from the port files), a
path that has a token equal to an id or name word (`foo_dt2.py`, `dt2/`), a file with the comment line `device-literals: allow` in
its first 5 lines, and in a `.toml` file the table headers and the values of `ports =` and `devices =`.

## Device levels

What a device file contains decides what the tools do with the device:

| Tables present | Level | Effect |
|---|---|---|
| `[device]`, `[[firmware]]`, `[capabilities]`, `[container]` | identity | firmware lookup by sha256, analysis tools |
| + `[patch]` (needs `[device] syx_id`) | patchable | patchkit and `port_resolve` accept the firmware |
| + `[digikit]` with `[board]`, `[limits]` (and `[card.store]` for a module store: optional) | runtime target | `patchkit gen` shared constants, runtime objects |
| + `[acceptance]` | test cell | the firmware, ready directory and sample-card input of the device tests |

`[digikit]` without `[patch]` is an error: a runtime target must be patchable. `[panel]` is optional.

`[capabilities]` says what the device has: `dsp` (`sharc` | `none`), `seal` (`hmac` | `none`), `sample_pool` (`stock` |
`ddr` | `none`) and `card` (`path-fs` | `slots` | `none` | `unknown`). Code branches on these values, not on device names.
`[container]` names the role of each syx section (`main`, `dsp`, `cf2`, `fpga`) and lists the `readonly` sections that are never
written; a section may have a role and be read-only, but `main` may not be read-only. `repack` lists roles of read-only sections that a module may store packed
when it declares that it unpacks them (`[[repack]]` of its `module.toml`, [PATCHING.md](../PATCHING.md) "Repacking a read-only section"); each must be a role with a read-only section. `devdata.rs` `validate` checks that
the capabilities agree with the other tables (for example `dsp = "none"` forbids a `[[firmware]].dsp` image and `container.dsp`).
`[patch] main_dest`, `main_stage`, `main_margin_min`, `boot_load`: where the bootstrap unpacks MAIN OS to (`main_dest`, 0x40000400), where it first stages the packed MAIN (`main_stage`: 0x40400000
on the Digitakt II and Digitone II, 0x40200000 on the Syntakt), the least in-place margin an image may have there (`main_margin_min`, bytes of unread stream ahead of the output, default 8192) and the
address the bootstrap is loaded at (`boot_load`, used by `tools/firmware/aplib_check.py`). patchkit stores MAIN with a literal tail when needed and refuses an image below the floor (finding 152).
`main_staged_max` is a different bound: the decoded MAIN may not reach the next 4 MiB boundary.
`[storage]` names the backend of each kind of content (`samples`: `stock-rpc`, `digikit` or `stock-slots`; `modules`: `digikit`); `@digi/devices` reads it, the samples code of the apps and `wb` branch on it, never on a device name. `[storage.slots]` (`stock-slots` only): `path`, `count`, `bytes_max` (audio bytes of one sample), `rate`, `channels`, `bits`, `byte_order`, `apply` (`restart`: the device reads the slots only at start).

Readers: Rust embeds the files (`packages/patchkit/build.rs`, `packages/emulator/device-profile`); TS and Nx use `@digi/devices`
(`packages/devices`, Node and Vite loaders); Python uses `tools/firmware/device.py`. The four readers do not share one validator:
each reads the tables it needs and checks those.

- patchkit (`devdata.rs`): `[device]`, `[[firmware]]`, `[capabilities]`, `[container]`, `[patch]`, `[digikit]`, `[board]`,
  `[card]`, `[limits]`, `[acceptance]`; it also checks that the tables agree with the capabilities.
- `device-profile` (the emulator): `[device]`, `[[firmware]]` (boot and DSP fields), `[panel]`, `[card]`, `[digikit]` (keep area),
  `[capabilities]`, `[container]`, `[acceptance]` and `[emulator]`; it refuses an unknown key in `[emulator]`.
- `@digi/devices`: `[device]`, `[[firmware]]`, `[capabilities]`, `[container]`, `[storage]`, `[patch]`, `[digikit]` (level, ids, default
  modules), `[acceptance]`; it checks the capability words, the level rule and that no id is described twice.
- `tools/firmware/device.py`: `[device]`, `[[firmware]]`, `[panel]`, `[capabilities]`, `[container]`, `[acceptance]` and
  `[emulator]` (kept as a dict, not checked), and whether `[digikit]` exists. It requires `[device] short`, checks the capability
  words against the same closed sets and refuses a repeated `short` or `syx_id`. The Python build tools read the rest of the
  file themselves (`tools/patchkit/pk_device.py`).

Faceplates and LED maps are not device-file tables. The web app and the workbench pair files by the stem of the device file
(`devices/<stem>.toml`): the faceplate is `apps/web/src/data/faceplates/<stem>.toml`, the LED map `devices/<stem>-leds.toml`. A
device has a faceplate when its faceplate file exists; the LED map is optional. The LED maps carry no `[device]` table, so
readers that scan `devices/` skip them. Which devices the public site offers is not a device fact: `SITE_DEVICES` in `apps/web/src/data/site-devices.mjs` is the one list that its pages, gallery, emulator pick and Transfer library follow.

## Where each fact lives

| Fact | Source (data) | Used by | How it gets there |
|---|---|---|---|
| Short name, long name, firmware list (sha256, version) | `devices/<dev>.toml` `[device]`, `[[firmware]]` | everything | read directly |
| DMA timer counters that the emulator runs free at the bus clock (a firmware that polls one without starting it) | `[emulator] free_running_dtcn` (channels 0..3; absent = none) | emulator boot (`device-profile` `free_running_dtcn_mask`); `DIGI_DTCN0=bus\|off` overrides channel 0 | read directly |
| SysEx device byte of the `.syx` framing | `[device] syx_id` | patchkit (`syx.rs`, `devdata.rs`) | embedded by `packages/patchkit/build.rs` |
| Device id and name digikit uses (runtime, `core/dynreg`, `core/dyndsp`, `.dkm`, the INFO reply) | `[digikit] id`, `name` (at most 3 characters) | patchkit `register.rs`, `dkmdsp.rs`; the device objects | `devdata.rs` (embedded); `DEVICE_ID`, `DEVICE_NAME_WORD` in `build/gen/device-<dev>.inc` |
| Module store: first sector, length (0 without `[card.store]`), header B, selection record copies | `[card.store]` | runtime (`dkrt_sdk::store`, `sel`), tools | `STORE_FIRST`, `STORE_SECTORS` in `build/gen/device-<dev>.inc`, laid out by `device.s` as `dkrt_store_first`, `dkrt_store_sectors`; Python: `tools/patchkit/pk_device.py` |
| Working-project sector (firmware area above the store) | `[[card.regions]]` role `working-project` | `pk_digikit_scen.py` | `pk_device.working_project_sector()` |
| Run-time machine type range (`dynamic_first ..= last`) and with it the number of reserved machine rows and voice rows | `image-modules/points/machines/types.toml` | runtime `dsp.rs`, `core/dynreg`, `core/dyndsp` | `dyn_type_first`, `dyn_types` of `dkrt_dev`; `DYN_TYPE_*` in the generated files of the other modules |
| DSP area size (window 0) and frame tail geometry | `[digikit] dsp_area_bytes`, `dsp_tail_off`, `dsp_tail_words` | `core/dsp-link` DSP object, `core/dyndsp` table | `DL_AREA_BYTES`, `DL_AREA_WORDS`, `DL_TAIL_*` in `build/gen/device-<dev>.inc` (SHARC `#define`s and gas `.equ`s) |
| Console UART, DDR window and uncached alias, eSDHC controller (absent: no `DEV_HAS_SDHC`) | `[board]` | `tramp.s` (soft reset), `fault.rs`, `api.rs` (`card_busy`), `platform.rs` (`word_at_literal`), `fx` service (`kit`) | `dkrt_dev` (below): `RealPlatform` fills `DeviceProfile` from it; a service takes its profile by device id |
| Safe-mode key (a firmware key index; absent: no `DEV_HAS_SAFE_KEY`) | `[digikit] safe_key` | runtime boot (`safe_mode.rs`) | `dkrt_dev`; `DeviceProfile::safe_key` |
| The tracks of the device (what a module that draws or counts tracks reads; absent: 0) | `[digikit] tracks` | modules, through the import `dk_device_track_count()` | `tracks` of `dkrt_dev`; `DeviceProfile::tracks` |
| The logical keys: the key names and their ids in the firmware's key events (the firmware's key index: equal to the panel code on the Digitakt II and Digitone II, different on the Syntakt, and not the wire code that `dk_input_key` injects; a name a device lacks maps to 0) | `[input.keys]` (`NAME = id`; at most 63 names in the SDK list) | modules name a key as `dk_key` (`DK_KEY_TRK`, `DK_KEY_TRIG(n)`); the `shortcuts` service translates a panel code to the logical key | `sdk/include/dk/gen/keys.h` (the numbers are the order of `[keys] order` in `sdk/sdk.toml`, which only grows at the end; a device file may use only names of that list, and the generator refuses another); `key_codes` of `dkrt_dev` and `DeviceProfile::key_codes`, indexed by the number |
| Where samples live and how long a sample path may be (the runtime branches on these, never on the device id) | `[capabilities] card` (`slots`: the store; `path-fs`: the firmware's folder filesystem), `[digikit] sample_path_max` | runtime `Platform::samples_in_store`, `sample_path_max` | `SAMPLES_IN_STORE`, `SAMPLE_PATH_MAX` in `device-<dev>.inc` (words `dkrt_samples_in_store`, `dkrt_sample_path_max` of `device.s`); `DeviceProfile` constants of `abi/src/profiles.rs` |
| RAM that the firmware and boot code leave alone and that the emulator keeps across a soft reset (the crash record, the keep probe) | `[digikit] keep_base`, `keep_bytes` | runtime `fault.rs`; emulator (`device-profile` `keep_ram`) | `dkrt_dev`; the emulator reads the file |
| The device's test cell: the firmware the acceptance tests run, the directory of the ready snapshot, whether a sample card is an input | `[acceptance] firmware`, `ready_dir`, `sample_card` | `tools/nx/device-inputs.mjs`, `packages/test-support/src/inputs.ts`, `tests/regression/runtime/devices.ts` | read directly; `ready_dir` is the one source of the ready directory (Python `digi_env.READY_JSON_DIRS`, `tools/nx/ready-dirs.mjs`, `@digi/devices`, `devdata.rs`) |
| The eMMC write guard: the switch and the allow-list rows (which sectors a stock caller and the runtime's raw write may write or erase; defaults `stock = [0, store.first)`, `runtime` = the store) | `[card.guard]` `enabled`, `[[card.guard.row]]` `who`, `first`, `end` | `core/card-guard` (hooks, `guard.h`), patchkit (`needed_when` rule, `apply`) | `CG_N`, `CG_FIRST_i`, `CG_END_i`, `CG_WHO_i` in `build/gen/card-guard-<dev>.inc` (kind `gas-card-guard`); `st/device.s` lays them out as `dkcg_rows`; `patchkit gen` checks the rows against the store and every `[card]` item |
| RAM ranges the PEEK command may read (at most four `[lo, hi)`: the firmware's data, the runtime's region with the keep area; no peripheral registers) | `[digikit] peek` | runtime `upload.rs` (`cmd_peek`) | `PEEK_N`, `PEEK_LO_i`, `PEEK_HI_i` in `build/gen/device-<dev>.inc`; `device.s` lays them out as `dkrt_peek_ranges` (the ranges differ per device, so they are a per-device object, not a shared constant) |
| DSP clock and blocks per second (so cycles per block) | `[board] dsp_clock_hz`, `dsp_blocks_per_second`. The clock word in the start-up code decodes to 1 GHz **[D]** ([finding 100](../findings/100-module-limits.md); the control-register write is not traced). | runtime HEALTH figures | `dsp_block_cycles` of `dkrt_dev` (rounded; 0 without a DSP) |
| Container and section limits of a port, DSP L2 range and its excluded ranges, MAIN OS load address | `[patch]` | `port_resolve.py` (written into `ports/*.toml` `[limits]`), patchkit `verify.rs` (an image with no port) | `port_resolve.load_profiles()`; `devdata.rs` |
| Address windows that only the firmware and `core/*` drive (flash controller, FPGA and peer buses); `[lo, hi)`, hi exclusive; a window starting below 0x10000 is data only | `[protect] windows` | patchkit `protect.rs` (pack time) | `DeviceInfo::protect` ([finding 141](../findings/141-syntakt-analog-and-nor-paths.md)) |
| User manual of the device (glob of its extracted pages under `out/refs/`; optional) | `[device] manual` | `tools/coldfire/atlas.py` | `Device.manual` in `tools/firmware/device.py` |
| Board facts that only the emulator uses (below, "The `[emulator]` table") | `[emulator]` | emulator boot, launcher, panel | `device-profile` fields; `Device.emulator` in Python |
| Which firmware has a port | `[[firmware]] port = true` | `port_resolve resolve-all` | read directly |
| Device list for manifests (`devices = [...]`), `pack --device` | all device files | `prim.rs`, `module.rs`, `main.rs` | `devdata::by_short` |
| Per-device knob and frame layout of machines | `[modules.knobs]`, `[modules.frame]` | typed sections of modules (`typed.rs`) | existing generator (`consts.rs`, `<dev>.inc`) |

## The `[emulator]` table

Board facts that only the emulator uses. Every key is optional; an unknown key is an error in `device-profile`.

| Key | Meaning |
|---|---|
| `free_running_dtcn` | Channels 0..3 of the DMA timer counters that run free at the bus clock, for a firmware that polls a counter without starting it. Absent: none. |
| `board_revision` | The PCB revision byte that MAIN reads from FlexBus CS1 at address 0. An emulator choice (the smallest value that passes the firmware's checks), not read from a unit. |
| `calibration_record` | What the emulated NOR holds at the calibration record offset. `"synthetic"`: a valid record built from the format alone, tables zero; never data from a unit. |
| `peer` | The second CPU. `"coldfire"`: its program runs on a second ColdFire core. `"mailbox-responder"`: no peer code; the emulator only answers MAIN's handshake. |
| `peer_audio` | Sub-table `[emulator.peer_audio]` (`dma_channel` 32..=63, `slots` two words below 16, `bits` 8..=32): MAIN mixes the peer's voices itself and streams them to the converter by eDMA. Needs `peer = "coldfire"`. The launcher builds the audio output (feature `play`, its own cargo target dir) and sets `DIGI_EMU_PEER_AUDIO` only for a device that has it. |
| `led_protocol` | How the panel UART carries LED state: `"indexed"` (default, `b9 idx val` records) or `"packed-states"` (palette records and packed 2-bit states). |

CPU-architecture registers of the MCF5441x itself (the UART status register offset, `RCM.RCR`, the eSDHC `PRSSTAT` offset) are
named constants in the code (`MCF5441X_*`): they are the same on every board that uses the chip. Which UART is the console, where
DDR is and which eSDHC holds the card are board choices and live in `[board]`.

## What `[[gen]]` does

A module asks for generated files with `[[gen]]` in its `module.toml` (`kind`, `path`, and `uses` for four kinds). `patchkit gen` writes them from the device
files and `types.toml`; `gen --check` fails when one is stale. The kinds (`packages/patchkit/src/devgen.rs`):

- `gas-dkdv`: the runtime's device object `dkrt_dev` (struct `DkrtDevice` of `runtime/core/abi.toml`: magic, version, size, `DEV_HAS_*`
  capability bits, then the console UART, the DDR window and alias, the eSDHC base, the safe key, the keep area, the DSP cycles per block and
  the run-time machine types), one file per device, as `.long` lines that `device.s` includes into its `.rodata`. The field order is the ABI
  file's; each field takes its value from a device constant (`DKDV_VALUE` in `devgen.rs`). `gas-dkdv-offsets`: `.equ DKDV_<FIELD>, offset`
  for assembly that reads the object (`tramp.s`), one file for all devices. The capability bits are derived: `DEV_HAS_DSP` from
  `[capabilities] dsp = "sharc"`, `DEV_HAS_STORE` from `[card.store]`, `DEV_HAS_SDHC` from `[board] esdhc_base`, `DEV_HAS_SAFE_KEY` from
  `[digikit] safe_key`; the facts of an absent capability are absent from the file (the validator refuses DSP facts when `dsp = "none"`).
- `gas-card-guard`: `.equ CG_*` lines for `core/card-guard` from `[card.guard]` (the rows after merging touching rows of one class; at most 8), one file per device; only for a device whose switch is on.
- `gas`: `.equ` lines, one file per device (`path` contains `{dev}`), for the device objects: `.include "device-dt2.inc"`.
- `sharc`: `#define` lines, one file per device, for DSP sources (`#include "device-dt2.inc"`). Each note is on its own line:
  the assembler's preprocessor keeps a trailing comment in a macro body, which swallows the rest of a line that uses the macro.
  `.PKBSS name, MACRO;` takes a `#define`d number as its size (`packages/patchkit/src/build/sharc_obj.rs`).

A module that a device switch makes necessary declares `[module] needed_when = [{ switch = "card.guard.enabled", with = "core/runtime" }]`: `switch` is a boolean of a nested table of the device file (`DeviceInfo::switches`), `with` another module. Where the switch is true, every patch set and every selection that holds `with` must hold this module; where it is not true, a set that holds this module is refused (`patchkit gen`, `apply`).

A module that reads a table of the device file declares it in `[module] needs_device` (dotted paths, for example `["input.keys"]` for `core/ui`, whose
`ui-<dev>.inc` holds the ids of its four keys and the flag bits). Every device file with a patch set that includes the module (as a root or through `requires`; a device with no
`[digikit]` too) must have the table, or `patchkit gen`, `patchkit build`, `patchkit pack` and the module's own generator refuse, naming the device, the patch
set, the module and the table. A module id in a patch set that the library lacks is an error. `[input.keys]` must name YES, NO, RIGHT and LEVEL_PUSH (the second YES key) for core/ui; `patchkit gen` refuses a device that names keys
without one of them. `[input.key_flags]` (required with `[input]`) gives the bit numbers (0 to 31, all different) of a key event's flags word by meaning: `press`
(set on a press, clear on a release), `func` (FUNC held), `repeat` (a held key repeating) and `release` (set on a release). Services and core/ui read them
(`DeviceProfile.key_flags`, `KEY_FLAG_*` of the generated constants); no code holds a flag literal.

The runtime object is built once and used on every device, so it has no device value in its code: what differs between devices is data of
`dkrt_dev` (and of the other per-device symbols of `device.s`: `dkrt_device_id`, `dkrt_store_*`, `dkrt_peek_*`), which the runtime reads through
`RealPlatform` as a `DeviceProfile`. A service has no access to the object: it takes the profile of its device from the generated table
(`DeviceProfile::by_id(core.device_id())`, `abi/src/profiles.rs`; the `rust-profiles` kind writes the table from the same facts as the object, so the two
cannot differ). The sizes of the runtime's static arrays (`ARENA_BYTES`, `STAGE_BYTES`, `MACHINE_ROWS` and the selection's `SEL_MAX`) are constants of
`abi.toml`; `gen` checks that the device files' `[limits]` agree with them, and the runtime checks at start that the device's machine type count fits
(`dev::valid`). The parameter rows of the image are device data too: the file's `[rows]` and `[[rows.block]]` (below) reserve them from `[limits]`, and `patchkit gen` writes `rows_info` and the cap words from them (`ROWS_*` in `core/rows` `build/gen/device-<dev>.inc`).

`[rows]` (only a device with a parameter table has it; the Syntakt has none and gets no `ROWS_*` constants):

| Key | Meaning |
| --- | --- |
| `recipe` | 15 words: how a row is made from the template's row (0 to 0x3c copy that word of the template; 0x100 min, 0x101 max, 0x102 default, 0x103 name pointer, 0x104 short pointer; 0x105 to 0x108 computed words) |
| `caps` | 4 `[limits]` keys whose values are the capacity words the admission reads, in the order of the module's `cap_<kind>` info entries |
| `[[rows.block]]` | exactly 3 blocks, in order: `owner` (the number the owner is known by), `limit` (a `[limits]` key), `per` (factor, default 1; rows = `per * limit`), `template` (the stock code whose row a free row copies) |

The block sizes have one source: a reservation `count` in a manifest reads them as `rows_b1`, `rows_b2`, `rows_b3` (the same `per * limit`), so the entries and `rows_info` cannot differ.

```toml
[rows]
recipe = [0x00, 0x108, 0x100, 0x101, 0x102, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x103, 0x2c, 0x104, 0x34, 0x38]
caps = ["machine_rows", "param_rows", "page_rows", "send_slots"]
[[rows.block]]
owner = 1
limit = "param_rows"
template = 0xd5
[[rows.block]]
owner = 2
limit = "fx_slots"
per = 8
template = 0x75
```
(a third block follows the same way)

`uses = [...]` is required on the kinds `gas` and `sharc`: it lists the constant names the module reads.
A trailing `*` matches a family by prefix (`FX_VALUE_*`). The generator writes only the listed names, in its own order. It refuses:

- a listed name that matches no constant;
- a listed name that the module's own sources never reference (whole word, comments count as references; the scan skips the
  directories `build`, `target`, `node_modules` and `tests`, directories whose name starts with `.`, and the generator's own
  output files; they are checked against `*.s *.S *.inc *.c *.h`).

A constant the sources use but the list lacks is not in the include, so the build fails on the undefined name.

The kinds `sharc-adapter`, `gas-card-guard`, `gas-sample-pool`, `sharc-sample-pool`, `c-shared` and `rust-profiles` take no `uses`. `rust-profiles` writes one `DeviceProfile` constant for each device file (the runtime ABI crate; `FakePlatform` of the host tests).

## Optional anchors

A manifest may list `optional_anchors = [...]` in `[module]`: names of anchors and hooks that the module can do without. When a port lacks
one, `patchkit` still takes the module: an object's link to the anchor is 0 (and the tail symbols `hook.<site>.tail` and `.return` of a
listed site are 0), a hook op whose `hook` or `anchor` is listed is skipped, and `plan.json` lists `<owner>:<name>` under `disabled`
(`tools/patchkit/image_report.py` prints `disabled: ...`). The module's code checks the link for 0 before it uses it. A name that the port
has is not disabled; a missing anchor that is not listed still refuses the module.

## Adding a device (what exists today, what is still hand work)

1. `devices/<name>.toml`: `[device]` (name, short, `syx_id`), `[[firmware]]` entries (add `port = true` when a port is wanted),
   `[capabilities]`, `[container]`, then the tables of the level the device needs (see "Device levels"): `[patch]`; `[board]`,
   `[digikit]`, `[card]` with `[card.store]`, `[limits]`; `[modules.knobs]`/`[modules.frame]` for machines; `[panel]`; `[acceptance]`.
   Rebuild patchkit (the device files are embedded). The code refuses a device whose `syx_id` or `[digikit] id` repeats.
2. For `seal = "hmac"`: the container seal key of the device in `SEALS` (`packages/patchkit/src/syx.rs`); `validate` refuses
   an `hmac` device without an entry. A device with `seal = "none"` needs no key.
3. `image-modules/points/machines/types.toml`: a `[<dev>]` table (`last`, `dynamic_first`, `alloc`, `dynamic`).
4. Modules that have per-device sources add an `[[object]]` with `devices = ["<dev>"]` and a small source for the device (the
   device objects of `core/runtime`, `core/dynreg`, `core/dyndsp`, `core/dsp-link` include the generated `device-<dev>.inc`).
   `core/dynreg` and `core/dyndsp` need `DYN_TYPES` rows each (`tests/patchkit/test_pk_device.py` checks it).
5. `patchkit gen`, `uv run python -m tools.patchkit.pk_build build-all`, `uv run python -m tools.patchkit.port_resolve resolve-all`.
   The anchors of a new firmware are rules in `ports/fw/` ([firmware-decls.md](firmware-decls.md)).
6. Still outside the data: `pk_harness.py` (firmware table, panel buttons), the `Fixed(...)` address tables of the emulator and the
   web app's device list (`apps/web/src/data/device-profiles.ts`).

Facts left in code on purpose: the DT2-only project seeding of `plusdrive` (a capability limit, not data), the container seal keys
(above), and the register offsets of the MCF5441x.
