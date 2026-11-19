# Port file: device and firmware specifics in one file built from committed data

Status: PR1 merged (PR #621); PR2 on branch `pf/pr2-runtime` and PR3 on `pf/pr3-workbench` (a stack: PR3 is merged from PR2, one device gate for both) are done on the host; the device proof is not run. PR2: table version 3 in `dkneeds`; role 2, the port steps, the search of `ports/` by `FW`, the import lookup, the installer on `SITES`, the commit filter, NEED `port`, INFO `fw_id`, runtime 0.10.0 and a new `SVC_ABI`, assembly cases, the `svc-test` hook and `providers.device.test.ts`, `PROVIDER_MAX` 8. PR3: the harness and Transfer put the packed services and the matching port file on a card, modules move to `modules/<kind>/<author>-<name>.dkm` (the one-time `RENAME` runs at the start of an install; a view only reads), and the binding is deleted. PR4 done on the host (branch `port/pr4-lfo`; the device proof is not run): `lfo.dks` is one file for both devices. The `SLOTS` items carry a device mask in bits 16 to 23 of their flags (bit `id - 1` of the runtime id), the loader skips an item for another device and drops it from the declared copy, and the packer no longer drops ColdFire fills; the two packs are the same bytes. No table or SDK version moved. Words, card paths and the repo tree: [taxonomy](../design/taxonomy.md). PR2 and PR3 start after PR D (lfo shapes) and E5 ([extension-model.md](extension-model.md)); both are merged.

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

It uses the container of `.dkm` and `.dks`: a third role (2, "port") with the same header, sha256, upload, verify and store path. It provides the name `port`: it is a provider, not a service. `patchkit pack` writes a hard NEED for `port` into every service that has a firmware reference. The boot planner orders the port file first and refuses its dependents when it is missing. Transfer installs it through the existing needs resolver and picks the file by the device's firmware identity. Safe mode skips it like any provider, so all sites stay stock.

Card path: `ports/<device>-<version>.dkport` (for example `ports/dt2-1.17.dkport`; `<version>` is the firmware version string), extension `.dkport`, role 2. A card belongs to one device. Port files of several firmware versions sit side by side. The match is on the `FW` hash, not on the file name: the runtime picks the file whose `FW` equals the image `fw_id` and ignores the others. With no match, the planner refuses every service that needs `port`. The planner never loads a non-matching file. `E_HOOK_STOCK` applies only to an explicit load of a non-matching port file (a test or tool). `services/port.dks` was never shipped, so there is no migration. The `FW` table item moves from services into the port file. The existing check against the image's `fw_id` stays (`modules/core/runtime/src/rs/src/loader/role.rs:106-126`).

Firmware identity in Transfer: the INFO reply carries no `fw_id` today. It has the device name (`port` of `DkInfoFull`, bytes 4-7), the id of the flashed image (`image_id`, bytes 33-40, `modules/core/runtime/src/rs/src/admin_cmds.rs`; `packages/dk-session/src/dkupload-core.ts:605`) and `svc_abi`. The image id changes with every patched build, so it cannot select a port file. PR2 adds `fw_id` (8 bytes, read from the `fw_id` info entry through `p.info`, `loader/role.rs:114`) to the INFO reply after `svc_abi`, with a capability bit, and `DkInfoFull.fw_id` in dk-session. Transfer picks the port file by `DkInfoFull.device` (today `port`; the field is renamed) and `fw_id`. With no device connected, the site shows the library; the port file is chosen at install, when a device is connected.

## 5. Port file contents

1. FWNAMES: name hash to address, for every anchor, exit and hook site that services may use. About 65 entries today: 45 hook references, 17 anchor aliases (among them the parameter table and the three WAVE rows of the lfo service) and at least 3 `dkfw` names (lfo, machines, fx). Source: every entry in `devices/fw/*.toml` whose `users` names a `core/services/*` id. The file lists every name known to the build. A name that the firmware of this port does not have (for example `cf.lfo.hook.wave_name3`, which only the Digitone II has) stays in the list with an `absent` flag and no address. `patchkit service-check` fails a service that references a name outside this set.
2. SITES: per hook site the address, the displaced length (whole instructions), a stock check value, the kind (raw, tail, stock) and an optional fixup list (reserved, empty today). A site of an absent name carries the `absent` flag.
3. Step 3 (section 9): the device-specific hook adapter routines, as code built from our sources.

Stock check: before it copies or installs, the runtime compares the first 8 bytes of the sha256 of the live site bytes with the port value. The `sha256` of a port anchor in `devices/ports` (for example `devices/ports/dn2-1.12.toml`) covers only the anchor length (2 to 8 bytes), so it cannot serve. The check value is a new committed field, computed over the displaced length of the site and written by `port_resolve` from local firmware. The runtime has `sha256.rs`.

Hook sites per device (counted from the `[[hook]]` entries of each service's `dks/module.toml`, with their `devices` filters; the installer table of `install_sites` 48 counts these sites, and the slot `frametype` is no site):

| Service | DT2 | DN2 |
| --- | --- | --- |
| lfo | 2 | 3 |
| machines | 7 | 10 |
| fx | 9 | 9 |
| Total | 18 | 22 |

Displaced instructions: only 7 (DT2) or 9 (DN2) of these hooks need a stock copy: lfo `graph_vec` (`tail`) and fx `page_desc` (`stock`), machines `source_key`, `source_turn`, `source_draw` and `source_code` (`tail`), `param_code.args` (DT2) or `param_code.map`, `type_page.count` and `type_page.desc` (DN2) (`tail`). All are plain copies: `patchkit port check` compares the copy that `bind` makes with the displaced bytes and `jmp.l` for every site, and the port resolve fails when a copied site is not a plain copy. The other 11 (DT2) or 13 (DN2), including the five branching sites, redo their instructions in the routine and exit through `dk_exit_` or `dk_ret_`. The first build has no fixup encoder. `patchkit` fails the port build when a site needs a form the runtime lacks. A form (`(d16,PC)` to abs.l, bra/bsr/bcc to 32-bit) is about 15 lines when first needed.

PR1 data (done): each hook of a service in `devices/ports/*.toml` carries `len`, `kind` and `check` (written by `port_resolve` through `patchkit port facts`) and a reserved `fixups` list (empty, refused by the port build). `stock = true` in `devices/fw` declares the `dk_stock_` hooks; the `users` of every name a service uses names the service (the port set is every such name). The `absent` flag is not committed: the port build derives it from the ports of the other devices (a name that only another device has). The port file is built by `patchkit port build` from committed data only (Nx `patchkit:ports`, `dist/ports/<device>-<version>.dks` today; now `.dkport`); its FWNAMES and SITES layout is in `packages/dkneeds/src/keys.rs` (tags 12 and 13, items of 12 and 20 bytes, header table version 3), which patchkit writes and the runtime reads. The file has a 4-byte image (`rts`, the target of every `dk_ret_`) and a bss for the copies of the tail and stock sites; the runtime fills them from flash after it compared the live bytes with the check value (`E_HOOK_STOCK`). A service file that holds `BIND` or `FW` is `E_FORMAT`. The assembly cases read the packed service files and the port file; the bound files of `patchkit bind` only give the stock bytes that the host test puts at the sites (until PR3 deletes the binding). A port file is about 2 KiB.

## 6. Runtime work and size

Port role steps; name lookup in the existing import seam (`loader/seams.rs:28-36`); tail copy from flash after the hash check; the installer takes site facts from SITES (`loader/install.rs` keeps the masked check, the `jmp.l` write, the flush and the restore); the commit step compares declared hooks with the HOOKS entries that have a port record. Size, measured on the runtime object (text 192518 bytes before, 196640 after, +4122; arena RAM: the port file's table copy and its bss, about 2 KiB with the 65 names and 22 sites):

| Group | Bytes |
| --- | --- |
| Port provider: check (`port_check` 528), place (356), `site_check` (256), `Sha::new` and `finish` outlined (386) | 1526 |
| Import lookup: `check_file` +524, `port_keys` 204, `Keys::name` 210, `find_name` and `find_site` 146, `parse_import` with `hex8` and `strip` about 180 | about 1260 |
| Hooks from `SITES`: `hook_items` 340, `filter_hooks` 256 | about 600 |
| Search by `FW`: `read_provider` 296, `plan_with_caps` +142, `table_of` +52, `has` 70 | about 560 |
| Readers and other: `Keys::site` 262, `role_ok` 282, `image_fw_id` 198, INFO +92, `load_inner` +98 | about 930 |
| Removed: `binds_of`, `Keys::bind`, the service `fw_check` and other bind code | about -1500 |

It stays in the runtime; the growth is accepted. Cuts that were measured or estimated and not taken: a bitmask `role_ok` (+84), dropping the search by `FW` (about -560), dropping the hash check of the copied sites (about -900, unsafe). It stays in the runtime. Removed: BIND parsing in services (`fw_check`, `binds_of`). Service hooks stay raw-only.

Who installs sites: the init of the port file builds the name table and the tails only. Each service's load installs its own hooks and takes the site facts from the port records. The one-owner rule of the planner and the install lifecycle do not change.

`PROVIDER_MAX` is 8 (`provider_max` in `sdk/sdk.toml`; it was 6). The service files are lfo, machines, fx and samples, the port file makes 5, and the test fixture `svc-test` and its second copy in `providers.device.test.ts` make 7. The raise from 6 to 8 costs 2361 bytes of runtime RAM and 426 bytes of text. By symbol (growth of the object sections, `runtime.dko.json`):

| Symbol | Bytes | Why |
| --- | --- | --- |
| `LOADER` (the loader static) | +1968 | per provider 984: the copy of the header table `Found::tables` 640, the claim row of the DSP installer 140, the slot words of the installer 128 (`SLOTS_MAX` 16 x 8), the path and its length 67, the table length and `ProvRec` 9; and 16 for `WANT_MAX` (4 x `PROVIDER_MAX` words) in all |
| `PROVIDER_PATHS` | +128 | 64 per provider |
| `DKRT_PROVIDERS` | +104 | 52 per provider (the module record) |
| `DKRT_DSP` | +104 | one DSP record per provider |
| `DKRT_CTXS` | +16 | one context per provider |
| other tables | +41 | small constant tables and padding |

The table copy of 640 bytes per provider is the largest part; `TABLE_MAX` is the planner's limit for one header table and stays. `LOADER` was in `.data`, not `.bss`: two of its fields started non-zero (`Installer::armed` true and `sel_of_rec` filled with 255), and one non-zero byte puts the whole static in `.data`, so the object carried 19.6 KB of zeros in its image. Both fields are stored inverted now (`disarmed`, index plus 1), `LOADER` is in `.bss`, and the runtime object holds 17.6 KB less data (`data` 31603 to 13996 bytes at PROVIDER_MAX 6 against 8 and zero fill; `data + bss` is 739775 bytes against 737414 before the raise). A static with a non-zero initial value must be checked in `runtime.dko.json` (`.data.` against `.bss.` sections). A search that finds no slot for a need or for the port file sets `Found::truncated`, and the boot logs one `MOD_LOAD` provider event with `E_PLAN_FIT` for each cause (`Found::truncated` bits: provider table full `(tab`, more files than `CAND_MAX` in `services/` or `ports/` `(fil`, more needs than `WANT_MAX` `(nee`; the log carries the first four characters, and `loadFailureText` of `runtime-log.ts` gives the text), so the user sees the cause and not a silent unmet need.

A production port file lists no hook of the test fixture: the fixture's hook sits on the site of `core/digikit-menu`'s hook, and `port_check` compares every copied site with its check value, which a patched image changes. `patchkit port build --with-test-hooks` (Nx `patchkit:ports-test`, `dist/ports-test/`) builds the port file of the test image, which runs without digikit-menu; the assembly tests and `providers.device.test.ts` use it. A host test (`patchkit` unit test) checks that no copied site of a production port file lies on a site that an image module patches.

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
| Service file bound by an older patchkit (`BIND` or `FW` item) | skipped by the search and logged as a refused provider: `E_SVC_ABI` (a Rust service, which carries the ABI word that changed) or `E_FORMAT` (a C service, ABI word 0) |
| Provider table full | `E_PLAN_FIT` in the provider log, with the unmet need left unmet |

## 8. Format and ABI

Table tags BIND (10) and FW (11) leave services. New port tags FWNAMES and SITES. `SVC_ABI` and the runtime minor version change once in this stack. The stack merges after E5 ([extension-model.md](extension-model.md)), which changes `SVC_ABI` first (`live_op`). Each change makes the runtime refuse old service files with `E_SVC_ABI`, and Transfer replaces them.

## 9. Step 3: adapter routines in the port file (after E8)

- The code section of the port file holds the device-specific hook routines that are in the services today.
- A service lists the hooks it takes in HOOKS without an entry. The planner one-owner rule and the install lifecycle stay as they are. The installer takes the entry from the port file by hook name. Image interrupt slots work the same way.
- Each routine calls the service through a pointer slot in the data of the port file. The service fills the slot by name with a neutral C function. These functions exist for lfo: `dk_lfo_eval`, `dk_lfo_name`, `dk_lfo_graph` (`modules/core/services/lfo/dks/src/hooks.rs:56-87`). They become a declared callback group in `sdk/sdk.toml` (name, signature, task or interrupt context). An empty slot takes the stock path that each routine already has.
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

PR D (lfo shapes) and E5 are merged, and so is PR1. PR2 and PR3 are one stack with one device subset gate and merge together; there is no dual path.

| PR | Content | Size | Confidence |
| --- | --- | --- | --- |
| 1 | patchkit and data: port fields (displaced length, kind, fixups) written by `port_resolve` from local firmware; `stock = true` declaration (bind infers `dk_stock_` today, `bind.rs:931-938`); role 2 with FWNAMES and SITES; `patchkit pack` for the port from committed data; a `--firmware` check that equals today's bind output once (site address, return, tail bytes, exits per service and device); Nx target | 6-8 h | high |
| 2 | Runtime and pack: the `ports/` search by `FW` (the runtime reads `ports/<device>-<fw>.dkport` files, picks the one that equals `fw_id`), port role steps, import lookup, tail copy, installer on SITES, commit filter, NEED `port`, BIND out of services, ABI and minor bump; assembly cases in `core/runtime:test-assembly` (fits, other firmware, missing name, port absent); the `test/svc-test` fixture's hook `cf.menu.hook.settings_list.tail` has no SITES item (its `users` lists only core/digikit-menu, and the fixture id does not match `core/services/`), and PR2 decides whether the port lists fixture hooks or the check skips them | 8-10 h | medium-high |
| 3 | Workbench and deletion: Transfer picks the port file by firmware identity and uploads it to `ports/`; `modulePath` is `modules/<kind>/<author>-<name>.dkm` (`dktransfer.ts:205`; the kind comes from the .dkm's records, as Transfer's group logic derives it; a changed kind moves the file) with the one-time rename of existing module files and the rewrite of the selection; the test harness puts the port file on the card in `ports/` (+3-4 h for this item); delete binding in `bind.rs` (keep import naming), `pk_bind`, `dkbind.ts`, `refuseUnbound`, `no-stock`, stock store use for services, Nx `bindTarget` and `dist/svc-bound`, the Nx target `@digi/web:bind-bundle` and its dependency in `@digi/desktop:workbench-inputs`, `apps/web/scripts/gen-bind.mjs` (and its call from `gen-modules.mjs`), `apps/web/public/modules/bind.json`, the `bind-failed` refusal kind with its `bind-data` hint, `BindUnavailableError` in `dkbind.ts`; docs | 9-12 h | medium-high |
| 4 | One `.dks` for all devices for services with no DSP section (lfo) | 2-3 h | medium |
| 5 | Step 3 after E8: lfo 6-8 h, fx 0.5-1 day, machines 1-2 days | | medium |

The `displaced` lines of the `patchkit bind` report for the copied sites on DT2 1.17 and DN2 1.12 confirm "copy only" (PR1).

Device subset, on DT2 and DN2: `S:lfo`, `S:machines`, fx contract and fxpages, `C:loading` (providers, refusal, safe-mode, transfer-live), `gate-smoke`, `gate-pins`. PCM pins must not move.

## 13. Risks

| Risk | Close with |
| --- | --- |
| A current tail needs the abs.l form | closed in PR1: none of the 7 (DT2) or 9 (DN2) copied sites does |
| `PROVIDER_MAX` has no room for one more provider | closed in PR1: 5 of 6 with the port file (section 6) |
| The workbench cannot learn the firmware identity without the `.syx` | INFO has no `fw_id` today; PR2 adds it (section 4) |
| Service and port file from different releases | `E_IMPORT` with the name hash (name not listed); Transfer uploads both |
| Import limit of a service file: `MAX_CF_IMPORTS` 128 (`loader/image.rs:94`, `packages/dkneeds/src/gen_limits.rs`) | read `H_IMPORT_N` (big-endian u32 at header byte 124) of the packed file: the unbound `machines.dks` has 59, `fx.dks` 37, `lfo.dks` 29 in this tree, so the limit is not near |
| A bad port file | a fault or hang before the stable period leaves the boot counter raised; two such boots in a row start the third in safe mode (`modules/core/runtime/src/rs/src/safe_mode.rs`, `ui_pass`), which skips the port file so all sites stay stock; replace it over USB |

## 14. Open follow-ups

- Syntakt: `devices/fw` rules already list `st` in `devices` (`menu.toml`, `rtos.toml`) and `devices/ports/st-1.42.toml` exists ([syntakt.md](syntakt.md)). The runtime runs on its main ColdFire (the peer ColdFire runs the stock peer program, and syntakt.md names no runtime or service on it). The port file applies the same way per ColdFire image that loads services. This is a follow-up of the Syntakt profile, not part of PR1-PR4.
- SHARC section in the port file. Evaluate after the lfo step-3 slice. It would put the DSP dispatch per device and firmware in the port file, so machines and fx become one file for all devices and future devices need no service change (this also covers ports for SHARC DSP). Cost: DSP memory whenever a port file is loaded.
