# Taxonomy: layers, terms, card layout, repo layout, names

This file fixes the words, the layers, the card layout and the target repository tree. [ARCHITECTURE.md](../ARCHITECTURE.md) describes the layers in detail; where the two differ, this file is the target and ARCHITECTURE.md follows it. The repository moves to the tree below in E8 ([extension model](../plans/extension-model.md)).

## Layers

| Layer | Holds | Must not hold | Calls and is called by | File |
| --- | --- | --- | --- | --- |
| Host | patchkit, workbench-core, desktop, site, wb, dk-session | device logic that exists nowhere else; imports of apps from below `apps/` | reaches the device only through SysEx, USB and the card | patched `.syx`, `dist/` |
| Modules (L4) | content, behaviour and own UI: machine, fx, filter, lfo-shape, tool | hooks, PROVIDES, KINDS and SLOTS items (`role_ok` refuses them); firmware names; device ids; another module's data | imports SDK functions by name; the runtime calls `init`, `fini`, `tick` and `menu`; the service calls it; UI work runs in the UI task only | `.dkm`, role 0 |
| Services (L3) | mechanism: record kinds, validation, lifecycle, exports, sticky numbers, the meaning of hooks, data per domain | firmware addresses and bytes; device literals; calls into another service; after port step 3, device routines | uses `CoreApi` and SDK imports; the core calls it through Service rows, modules call its exports by name, stubs and port routines call it through slots it fills | `.dks`, role 1; crate `dkrt-svc-<x>` and `dks/` |
| Port | the facts of one device and one firmware: anchors, hook sites with check values, the firmware identity; after step 3 the hook routines of all services | service logic, state, record kinds, firmware bytes | read by the loader; a routine reaches a service only through a pointer slot | `.dkport`, role 2 |
| Runtime (image, L2 and L1) | the kind-free core (load, plan, store, settings, transport, safe mode, tick, DSP load), platform modules, the installer, the image halves of services, adapters, fixes | a machine, effect or service name in the core; a hook that a `.dks` could own | reaches stock firmware through bindings; goes up only through declared Service rows and slots | patched firmware from committed `.dko.json` |
| Stock (L0) | Elektron firmware, DSP image, Elektron files | any digikit value that stock code cannot handle | | |

What can change while the device runs: content modules (E5, E5c). A service can be loaded but not replaced, until port step 3 and S3. The port file and the image need a restart or a flash.

## Terms

One meaning per word. A second use gets a new word.

| Term | Meaning |
| --- | --- |
| runtime | the base in every digikit image. "Runtime core" is `core/runtime`. "Everything we flash" is the image. |
| image | the patched firmware only. Qualify the others: module code image, DSP image, card image. The repo folder for image modules is `image-modules/`. |
| image module | a `module.toml` unit patched into the image. Its role gives its class. |
| platform | the `Platform` trait and the image modules behind it. |
| adapter | an image module for one device function that is no service half and no runtime part: usb-bulk, plusdrive-write, sdhc-large. ARCHITECTURE.md still uses "adapter" for all L1 image code (platform glue, hooks) until E8 renames the classes; read it as "image module" there. |
| image half | the part of a service that must be in the image (class `image-half`). |
| stub | image code at a site that calls a filled pointer slot. The fallback for an absent optional import is the "absent-import fallback". |
| slot | a pointer word that a service fills. Always qualified (`<service>_<site>`). |
| port | one device with one firmware version: id `<device>-<version>`, identity `fw_id`. `DkInfoFull.port` (today the device name) is renamed `device`. A MIDI port is a "MIDI port". |
| port file | the role-2 file of one port. |
| service | a named, versioned L3 unit that owns record kinds, exports and hooks. `type = "service"` on content modules goes. The import groups of sdk.md are "API groups". SysEx service commands are "module commands". |
| provider | a file that the boot plan orders and refuses by a name it provides: a service file or the port file. |
| module | a content module: `.dkm`, role 0, from the card. The other units are "image module" and "service file". |
| module kind | derived from the keys: machine, fx, filter, lfo-shape, tool. One vocabulary: the template names. `category` takes the same value; `type` goes. |
| record kind | a type in `[records.kinds]`, owned by one service. Never a bare "kind". |
| store | the file area on the card. |
| settings | the key/value record of device scope. It replaces "digikit's store" for that record. |
| selection | the ordered module paths loaded at boot. A project's list is "project modules". |
| project | an Elektron project (a card slot). The `.dkp` is the "project file". An Nx project is always "Nx project". |

## Card layout

```text
<store root>                   card: the area of [card.store]; host mirror: plus_drive/<device>/digikit/
  modules/<kind>/<author>-<name>.dkm   file name = module id with "/" as "-"
  services/<service>.dks       stem = service name
  ports/<device>-<version>.dkport   <version> is the firmware version string; picked by the FW hash == fw_id
  projects/<number>-<name>.dkp number 001..128 (slot + 1); header: device, slot, name, save stamp
  projects/archive/<name>-<stamp>.dkp   archived project files of the project service, kept for good; stamp as 8 hex digits ([digikit-project](digikit-project.md))
  samples/<folder>/<name>      devices with card = "slots" only
  data/<module id>/...         reserved for files that a module writes; not enforced
  logs/                        reserved
fixed sectors outside the table, two copies each: the settings record, the selection record
```

- A project file is `projects/<number>-<name>.dkp`. The number (firmware slot + 1, as the menus show it) is the identity, the name is for reading ([plan](../plans/digikit-project.md), Architecture "File name and identity").
- The folder shows the role. The role word in the header decides. A file in the wrong folder is not found.
- Port files of several firmware versions sit side by side. `<version>` is only the firmware version string, for people; the match is on the `FW` hash (the first 8 bytes of the stock image's SHA-256), which must equal the image `fw_id`. The runtime ignores the other files. With no match, the planner refuses every service that needs `port`. It never loads a non-matching port file.
- Module files are grouped by module kind (machine, fx, filter, lfo-shape, tool). The file name is the module id with "/" replaced by "-" (`modules/machine/fadedlimes-analog.dkm`, `modules/lfo-shape/digikit-lfo_shapes_1.dkm`, `modules/tool/digikit-remote.dkm`; `tools/bootscreen` is `modules/tool/tools-bootscreen.dkm`). The author is in the name, so two authors can use the same module name. Path length: at most 8 (`modules/`) + 10 (kind and `/`) + 31 (id) + 4 (`.dkm`) = 53 characters, under the 55 of a selection entry (`SEL_PATH` 56 with the NUL, `gen_abi.rs`).
- Trade-off: the kind is derived from a module's records at pack time. If a later version of a module changes its kind, Transfer moves the file to the new folder with the same one-time RENAME it uses for the migration, and rewrites the selection.
- Scopes: device scope is the settings record. The runtime offers a settings overlay over module keys, and the project service is its only user: while it is loaded a module's keys are also per project, the `.dkp` holds the entries it wrote there and a read prefers them. Keys are `<owner>/<key>`, with a service name or a module id as owner. Elektron files stay stock.
- A store belongs to one device. Each structure keeps its own version; paths carry none.
- Migration: Transfer renames existing module files once with the store's RENAME and rewrites the selection. Sticky maps and settings are keyed by module id, not by path, so nothing else moves. `services/port.dks` was never shipped.
- To check in the port-file PR 3: `store::list("modules/")` and the DIGIKIT menu with one more folder level.
- A content module can write any store path, including `services/` (`DK_FILES_STORE` has no prefix). `data/<module id>/` and `logs/` are reserved, and confinement is not enforced: usb-fast writes modules and samples on purpose.

## Repo layout

Target tree (E8):

```text
runtime/{core, platform/<name>, apps/digikit-menu}
services/<x>/{src, tests}      crate dkrt-svc-<x>
services/<x>/dks/              the .dks module
services/<x>/image-halves/<half>/   the image halves of the service
services/rows/                 today services/params; the crate is renamed to dkrt-svc-rows in a later PR
ports/{<device>-<fw>.toml, fw/, bindings/, routines/<service>/}
image-modules/{adapters, fixes, proofs}
modules/<kind>/<module id>/    content only; the folder is the id (<author>/<name>); kind folders: machine, fx, tool, lfo-shape, filter
devices/*.toml  sdk/  packages/  tools/  apps/  tests/  docs/          unchanged
```

| Today | Target |
| --- | --- |
| `modules/core/runtime` | `runtime/core` |
| `modules/core/expansion`; `modules/core/adapters/{platform,dsp-link,dyndsp,ui,health,input}` | `runtime/platform/<name>` |
| `modules/core/apps/digikit-menu` | `runtime/apps/digikit-menu` |
| `modules/core/services/{machines,fx,samples,lfo}` | `services/<x>` |
| `modules/core/services/params` | `services/rows` (the folder; the crate keeps its name until the later rename PR) |
| adapters `machines`, `params`, `dynreg`, `dsp-voice` | `services/machines/image-halves/` |
| adapters `fx`, `dynfx`, `fxpages` | `services/fx/image-halves/` |
| adapter `sample-pool` | `services/samples/image-halves/` |
| adapter `lfo` | `services/lfo/image-halves/` |
| adapters `usb-bulk` (with `usb-bulk-audio` merged in), `plusdrive-write`, `sdhc-large` | `image-modules/adapters/` |
| adapters `overdrive`, `introblank` | `image-modules/fixes/` |
| adapters `storage-fault-proof`, `file-catalog-proof` | `image-modules/proofs/` |
| adapter `dsp-stereo-proof` | the tests of `dsp-voice` |
| `devices/ports`, `devices/fw`, `modules/core/fw` | `ports/`, `ports/fw`, `ports/bindings` |
| `modules/machines/<name>` (analog, bd, pd: id `fadedlimes/<name>`; pluck, shot: `digikit/<name>`; tb303: `tom/tb303`) | `modules/machine/<module id>` |
| `modules/fx/freqshift` (`digikit/freqshift`) | `modules/fx/digikit/freqshift` |
| `modules/lfo/lfo_shapes_1` (`digikit/lfo_shapes_1`) | `modules/lfo-shape/digikit/lfo_shapes_1` |
| `modules/tools/<name>` (debug, probe, remote, usb-fast: `digikit/<name>`; bootscreen: id `tools/bootscreen`) | `modules/tool/<module id>`; bootscreen is the one id whose first part is not its author (author `digikit`), so its folder is `modules/tool/tools/bootscreen`; its id stays |

What does not move or change name: module ids, Nx project names (a module id or a crate name), crate names (the rename of crate `dkrt-svc-params` to `dkrt-svc-rows` is a later PR with an objdiff proof, because Rust symbol names may reach the objects), the `dkrt-sdk` name, `tests/fixtures/modules`, `devices/*.toml`, the text of findings and handovers.

Code that reads the paths and must change with the move: `tools/nx/modules.mjs` (the glob and `startsWith('modules/')`), `tools/devtools/layercheck.py`, literal paths in `modules/core/runtime/project.json`, Cargo path dependencies, the patchkit manifest roots. Before the move, check that the Rust objects embed no source path.

Roles in `module.toml` after the move: `runtime`, `installer`, `platform`, `image-half` (with `service = "<x>"`), `adapter`, `fix`, `proof`, `app`, `service`.

## Naming rules

These extend [sdk.md](../plans/sdk.md) section 10.

| Area | Form |
| --- | --- |
| SDK contract | `dk_<group>_<verb>`, `dk_<thing>`, `DK_<GROUP>_<NAME>` |
| Runtime core | C API `dkrt_<noun>_<verb>`, statics `DKRT_<NAME>`, crates `dkrt-<part>` |
| Service | crate `dkrt-svc-<x>`, file `<x>.dks`, id `core/services/<x>`; exports for the core only: `dk_<x>_<what>` with `internal = true` |
| Image module | id `core/<name>`; symbols with the module's short prefix; slots `<service>_<site>` (after port step 3 the device part goes: `lfo_dt2_tick_call` becomes `lfo_tick_call`) |
| Port | anchors `cf.<area>.<class>.<name>`; imports `dk_fw_`, `dk_exit_`, `dk_ret_`, `dk_stock_`, `dk_tail_`; routines `port_<service>_<hook>`; callbacks into a service `dk_<service>_<verb>` in a declared group |

Rules:

1. A name shows the layer that owns the thing.
2. Never a bare kind, key, slot, image, port, project or service where two meanings are possible.
3. A record kind is singular snake case, unique, with one owner service and a constant `KIND_<NAME>`.
4. Settings keys and `.dkp` sections start with the owner: `<service>/...` or `<author>/<name>/...`. An author may not be a service name, `core` or `test`; patchkit refuses it.
5. A module id is `<author>/<name>`. The repo folder is `modules/<kind>/<id>`; the card path is `modules/<kind>/<author>-<name>.dkm`. (`tools/bootscreen` predates the rule: its first part is not its author. Its id stays.)
6. Module kind names are the template names. `category` has the same value where it can change (E8 checks that). `type` goes.
7. Extensions: `.dkm` role 0, `.dks` role 1, `.dkport` role 2, `.dkp` project file.

## Order

1. This document and the plan edits.
2. E5 (merged).
3. Port file PR 2 (search of `ports/` by `FW`) and PR 3 (port pick in Transfer, `modulePath` = `modules/<kind>/<author>-<name>.dkm`, the rename migration, the harness cards; about 3-4 h more than planned).
4. E5c and E5b. They do not depend on the layout and come before E8.
5. E8, the top-level lift. The merge gate is a byte manifest (it proves no behaviour change), the host stage, `mise run test:plan`, gate-smoke and one device test per device, with the skips named in the PR ([extension model](../plans/extension-model.md) E8). The full device set reruns in the next batch's `test:full`. Merge `test/t1i-rig-read-hook` first (it edits `dsp-stereo-proof`).
6. After E8: port step 3 (`ports/routines/`), the project service ([digikit project](../plans/digikit-project.md)) and E7.

Known limits: all routines of one port file must fit `stage_bytes` 131072. The card and store facts of Syntakt are not read for this document.
