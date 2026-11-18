# Port file: device and firmware specifics in one file built from committed data

Status: design, todo. Stack PR1-PR3 and PR4 start after PR D (lfo shapes) and E5 ([extension-model.md](extension-model.md)).

## 1. Problem

- A service `.dks` is packed without firmware and names firmware through imports `dk_fw_`, `dk_exit_`, `dk_tail_`, `dk_stock_` and `dk_ret_` (`packages/patchkit/src/bind.rs:58-84`). `patchkit bind` resolves the names for one stock firmware. It adds trampolines, `BIND` items with the displaced stock bytes, and an `FW` item. A bound file holds firmware bytes, so it is never committed or published.
- The workbench binds at install time from the user's stock `.syx` (`packages/workbench-core/src/modules/dkbind.ts`, `dktransfer.ts` `bindService`, `tryBind`, `refuseUnbound`; refusal kind `no-stock`; `firmware/stock-store.ts`). Without the `.syx`, every module that needs a service is refused, also in the emulator.
- Services hold device-specific hook routines (`hooks-dn2.s`, `hooks-dt2.s`, `hooks-ui.s`: registers, frame offsets, redone stock instructions, constants) and per-device hook lists. Device specifics leak above the runtime layer.

## 2. Goal

Beyond the runtime, modules and services hold no device or firmware specifics. A user ticks any module. Install uploads it and the services it needs. No firmware file is needed and no service error appears. A new ordinary hook or anchor needs no reflash.

## 3. Rejected options

- A bind table in the image written by `patchkit apply`: a new hook needs a reflash.
- On-device bind with a general instruction decoder: not needed.
- A separate `.dkp` format with its own loader: a parallel path.
- Site data inside each `.dks`: every service becomes firmware-specific in data.
- Stock bytes fetched from the device or emulator, to keep host bind: keeps bound files and all bind code.
- A loader service for the new code: about 1 KiB does not justify a second unit.

## 4. Decision

One port file per device and firmware version. We build it from committed data only (`devices/ports` anchors and hashes, `devices/fw` sites) and our sources. It holds no firmware bytes and builds on CI without firmware.

It uses the container of `.dkm` and `.dks`: a third role (2, "port") with the same header, sha256, upload, verify and store path. It provides a service `port`. `patchkit pack` writes a hard NEED for `port` into every service that has a firmware reference. The boot planner orders the port file first and refuses its dependents when it is missing. Transfer installs it through the existing needs resolver and picks the file by the device's firmware identity. Safe mode skips it like any provider, so all sites stay stock.

Card path: `services/port.dks`. A card belongs to one device. `services/port.dks` is replaced when the firmware changes: Transfer uploads the matching file, and a port file for another firmware is refused with `E_HOOK_STOCK`, never used silently. The `FW` table item moves from services into the port file. The existing check against the image's `fw_id` stays (`modules/core/runtime/src/rs/src/loader/role.rs:106-126`).

Firmware identity in Transfer: the INFO reply carries no `fw_id` today. It has the device name (`port` of `DkInfoFull`, bytes 4-7), the id of the flashed image (`image_id`, bytes 33-40, `modules/core/runtime/src/rs/src/admin_cmds.rs`; `packages/dk-session/src/dkupload-core.ts:605`) and `svc_abi`. The image id changes with every patched build, so it cannot select a port file. PR2 adds `fw_id` (8 bytes, read from the `fw_id` info entry through `p.info`, `loader/role.rs:114`) to the INFO reply after `svc_abi`, with a capability bit, and `DkInfoFull.fw_id` in dk-session. Transfer picks the port file by `port` and `fw_id`. With no device connected, the site shows the library; the port file is chosen at install, when a device is connected.

## 5. Port file contents

1. FWNAMES: name hash to address, for every anchor, exit and hook site that services may use. About 65 entries today: 47 hook references, 13 anchor aliases and at least 3 `dkfw` names (lfo, machines, fx). Source: every entry in `devices/fw/*.toml` whose `users` names a `core/services/*` id. The file lists every name known to the build. A name that the firmware of this port does not have (for example `cf.lfo.hook.wave_name3`, which only the Digitone II has) stays in the list with an `absent` flag and no address. `patchkit service-check` fails a service that references a name outside this set.
2. SITES: per hook site the address, the displaced length (whole instructions), a stock check value, the kind (raw, tail, stock) and an optional fixup list (reserved, empty today). A site of an absent name carries the `absent` flag.
3. Step 3 (section 9): the device-specific hook adapter routines, as code built from our sources.

Stock check: before it copies or installs, the runtime compares the first 8 bytes of the sha256 of the live site bytes with the port value. The `sha256` of a port anchor in `devices/ports` (for example `devices/ports/dn2-1.12.toml`) covers only the anchor length (2 to 8 bytes), so it cannot serve. The check value is a new committed field, computed over the displaced length of the site and written by `port_resolve` from local firmware. The runtime has `sha256.rs`.

Hook sites per device (counted from the `[[hook]]` entries of each service's `dks/module.toml`, with their `devices` filters; the installer table of `install_sites` 48 counts these sites, and the slot `frametype` is no site):

| Service | DT2 | DN2 |
| --- | --- | --- |
| lfo | 3 | 4 |
| machines | 7 | 10 |
| fx | 9 | 9 |
| Total | 19 | 23 |

Displaced instructions: only 7 of these hooks per device need a stock copy (lfo `graph_vec` tail, lfo `t1_range` stock, fx `page_desc` stock, machines `source_key`, `source_turn`, `source_draw` and `source_code` tails). All 7 are plain copies by their declared text. The other 12 (DT2) or 16 (DN2), including the five branching sites, redo their instructions in the routine and exit through `dk_exit_` or `dk_ret_`. The first build has no fixup encoder. `patchkit` fails the port build when a site needs a form the runtime lacks. A form (`(d16,PC)` to abs.l, bra/bsr/bcc to 32-bit) is about 15 lines when first needed.

## 6. Runtime work and size

Port role steps; name lookup in the existing import seam (`loader/seams.rs:28-36`); tail copy from flash after the hash check; the installer takes site facts from SITES (`loader/install.rs` keeps the masked check, the `jmp.l` write, the flush and the restore); the commit step compares declared hooks with the HOOKS entries that have a port record. Size: +0.5 to +1 KiB text on about 216 KiB, under 1 KiB arena RAM. It stays in the runtime. Removed: BIND parsing in services (`fw_check`, `binds_of`). Service hooks stay raw-only.

Who installs sites: the init of the port file builds the name table and the tails only. Each service's load installs its own hooks and takes the site facts from the port records. The one-owner rule of the planner and the install lifecycle do not change.

`PROVIDER_MAX` is 6 (`sdk/sdk.toml:1862`). The port provider needs one more slot, so it becomes 7. `SLOT_WORDS_MAX` (`loader/install.rs:53`) is `SLOTS_MAX` (16) times `PROVIDER_MAX`: 96 entries of 8 bytes (768 bytes) become 112 entries (896 bytes), +128 bytes of RAM in the installer. `REC_MAX` and the provider rows grow by one entry each.

`BIND_MAX` leaves with BIND: PR2 deletes `bind_max` in `[limits]` of `sdk/sdk.toml` (line 1863), the generated `BIND_MAX` (`packages/dkneeds/src/gen_limits.rs`, `packages/dk-session/src/gen_proto.ts`) and the runtime copy (`loader/role.rs`).

## 7. Refusals

All use existing codes at MOD_LOAD. A name that the firmware lacks is not a refusal: one `lfo.dks` for all devices (PR4) declares `cf.lfo.hook.wave_name3`, which only DN2 has, and loads on DT2 with that hook skipped.

| Case | Code |
| --- | --- |
| Name not listed in the port file at all (a port file older than the service) | `E_IMPORT` (detail: name hash) |
| Name listed with the `absent` flag | no refusal: the loader skips the hook and resolves an import of the name to 0, as `bind` does today (`packages/patchkit/src/bind.rs:70`) |
| Live site bytes differ | `E_HOOK_STOCK` |
| Overlapping sites | `E_PLAN_OWNER` |
| Full site table | `E_PLAN_FIT` |
| Port file for another firmware | `E_HOOK_STOCK` |
| Port file missing | the planner refuses the dependents |

## 8. Format and ABI

Table tags BIND (10) and FW (11) leave services. New port tags FWNAMES and SITES. `SVC_ABI` and the runtime minor version change once in this stack. The stack merges after E5 ([extension-model.md](extension-model.md)), which changes `SVC_ABI` first (`reload_phase`). Each change makes the runtime refuse old service files with `E_SVC_ABI`, and Transfer replaces them.

## 9. Step 3: adapter routines in the port file (after E8)

- The code section of the port file holds the device-specific hook routines that are in the services today.
- A service lists the hooks it takes in HOOKS without an entry. The planner one-owner rule and the install lifecycle stay as they are. The installer takes the entry from the port file by hook name. Image interrupt slots work the same way.
- Each routine calls the service through a pointer slot in the data of the port file. The service fills the slot by name with a neutral C function. These functions exist for lfo: `dk_lfo_eval`, `dk_lfo_name`, `dk_lfo_range_max`, `dk_lfo_graph` (`modules/core/services/lfo/dks/src/hooks.rs:56-87`). They become a declared callback group in `sdk/sdk.toml` (name, signature, task or interrupt context). An empty slot takes the stock path that each routine already has.
- One port file per device and firmware holds the routines of all services.
- Hot reload: the site jump never points into a service, so a service replace is a slot swap. The entry and exit counter is a macro around the callback call in our adapter source. The runtime builds no gates. Replacing the port file needs a restart ([digikit-hot-reload.md](digikit-hot-reload.md)).
- Order: lfo first (with E6e item 2), then fx, then machines. Decide fx and machines from the size and cost of the lfo slice.
- Until the follow-up in section 14, machines and fx keep per-device SHARC dispatch, so those two services stay one file per device.
- A call from a port routine into a service goes through a pointer slot that the service fills ([ARCHITECTURE.md](../ARCHITECTURE.md) layer rules).

## 10. Decisions

- Ordinary hook sites and anchors need no reflash: a new port file and service go over USB, then the device restarts. New interrupt sites and SHARC stubs still need an image change ([runtime-and-services.md](runtime-and-services.md) 5.10).
- SHARC dispatch stays in the machines and fx services now.
- Step 3 as in section 9, with one port file per device and firmware for all services.
- A truncated sha256 of each site in a published file, and retyped stock instructions in published adapter code (existing practice in unbound `.dks`), are acceptable under the firmware-bytes rule.

## 11. Cost of a new port or device

- New firmware version: `patch:port` writes the port TOML and the port file is built from it. After step 3, adapter sources change only where frames changed. Service files do not change.
- New device: profile, ports, image adapters and a port file. Machines and fx need SHARC dispatch objects until the follow-up in section 14.

## 12. PRs

PR D (lfo shapes) lands first. PR1-PR3 are one stack with one device subset gate and merge together; there is no dual path. Build the stack after E5 merges, or in parallel and rebase: both touch the runtime loader (E5 unload and reload; the port file import lookup and installer).

| PR | Content | Size | Confidence |
| --- | --- | --- | --- |
| 1 | patchkit and data: port fields (displaced length, kind, fixups) written by `port_resolve` from local firmware; `stock = true` declaration (bind infers `dk_stock_` today, `bind.rs:931-938`); role 2 with FWNAMES and SITES; `patchkit pack` for the port from committed data; a `--firmware` check that equals today's bind output once (site address, return, tail bytes, exits per service and device); Nx target | 6-8 h | high |
| 2 | Runtime and pack: port role steps, import lookup, tail copy, installer on SITES, commit filter, NEED `port`, BIND out of services, ABI and minor bump; assembly cases in `core/runtime:test-assembly` (fits, other firmware, missing name, port absent) | 8-10 h | medium-high |
| 3 | Workbench and deletion: Transfer picks the port file by firmware identity; delete binding in `bind.rs` (keep import naming), `pk_bind`, `dkbind.ts`, `refuseUnbound`, `no-stock`, stock store use for services, Nx `bindTarget` and `dist/svc-bound`; the test harness puts the port file on the card; docs | 6-8 h | medium-high |
| 4 | One `.dks` for all devices for services with no DSP section (lfo) | 2-3 h | medium |
| 5 | Step 3 after E8: lfo 6-8 h, fx 0.5-1 day, machines 1-2 days | | medium |

Before PR2, the `displaced` lines of the `patchkit bind` report for the 7 sites on DT2 1.17 and DN2 1.12 must confirm "copy only".

Device subset, on DT2 and DN2: `S:lfo`, `S:machines`, fx contract and fxpages, `C:loading` (providers, refusal, safe-mode, transfer-live), `gate-smoke`, `gate-pins`. PCM pins must not move.

## 13. Risks

| Risk | Close with |
| --- | --- |
| A current tail needs the abs.l form | the bind report for the 7 sites |
| `PROVIDER_MAX` has no room for one more provider | raise 6 to 7 (section 6); +128 bytes RAM |
| The workbench cannot learn the firmware identity without the `.syx` | INFO has no `fw_id` today; PR2 adds it (section 4) |
| Service and port file from different releases | `E_IMPORT` with the name hash (name not listed); Transfer uploads both |
| Import limit of a service file: `MAX_CF_IMPORTS` 128 (`loader/image.rs:94`, `packages/dkneeds/src/gen_limits.rs`) | read `H_IMPORT_N` (big-endian u32 at header byte 124) of the packed file: the unbound `machines.dks` has 59, `fx.dks` 37, `lfo.dks` 29 in this tree, so the limit is not near |
| A bad port file | a fault or hang before the stable period leaves the boot counter raised; two such boots in a row start the third in safe mode (`modules/core/runtime/src/rs/src/safe_mode.rs`, `ui_pass`), which skips the port file so all sites stay stock; replace it over USB |

## 14. Open follow-ups

- Syntakt: `devices/fw` rules already list `st` in `devices` (`menu.toml`, `rtos.toml`) and `devices/ports/st-1.42.toml` exists ([syntakt.md](syntakt.md)). The runtime runs on its main ColdFire (the peer ColdFire runs the stock peer program, and syntakt.md names no runtime or service on it). The port file applies the same way per ColdFire image that loads services. This is a follow-up of the Syntakt profile, not part of PR1-PR4.
- SHARC section in the port file. Evaluate after the lfo step-3 slice. It would put the DSP dispatch per device and firmware in the port file, so machines and fx become one file for all devices and future devices need no service change (this also covers ports for SHARC DSP). Cost: DSP memory whenever a port file is loaded.
