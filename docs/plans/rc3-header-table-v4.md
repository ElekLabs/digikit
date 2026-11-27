# RC3 header table version 4: proposal

Date: 2026-11-22. Status: **proposal; decisions taken by Em 2026-11-22 (below).** Item RC3 of [runtime-contract-and-lifecycle.md](runtime-contract-and-lifecycle.md). Static read of the tree at `c3af7d3a1`; nothing was built or run.


Source: lane `fx-route-policy` at c3af7d3a1 (working tree equals the commit except the plan file). Paths are relative to the lane root. Read only; nothing was built or run. [D] = read once, not proven.

## 1. Today's DKTB table

Layout: `packages/dkneeds/src/keys.rs:1-47` (spec), tags `keys.rs:61-77`. Head: u32 `DKTB`, u16 version, u16 n_entries; entries in ascending tag order, each tag once: u16 tag, u16 count, u32 payload bytes, payload (BE, 4-byte aligned). Header words 0x90/0x94 hold offset and size.

| Tag | Name | Item |
| --- | --- | --- |
| 1 | NEEDS | 12: service hash, min version, flags (bit0 hard) |
| 2 | USES | 8: resource id (`[records.resources]`, global 1..6), count |
| 3 | KEYS | groups: u32 kind (global), u32 n, n u32 keys |
| 4 | KINDS | 12: kind, record len, handler entry |
| 5 | SLOTS | 8 (v1) or 12 (v2+): slot id, flags (device mask bits 16-23, `SLOT_DSP` 0x100), entry |
| 6 | HOOKS | 8: hook hash, entry; top byte of entry = device mask, low 24 = image offset (`HOOK_ENTRY_MASK`, keys.rs:84) |
| 7 | EXPORTS | 8: name hash, entry (bit31 bound, bit30 query) |
| 8 | RESOURCES | 8: resource id, limit |
| 9 | PROVIDES | 8: service hash, version (v2+) |
| 10 | BIND | variable: site hash, addr, stock len, tramp offset, stock bytes (v2+) |
| 11 | FW | 8: first 8 bytes of stock SHA-256 (v2+) |
| 12 | FWNAMES | 12 (v3, port file) |
| 13 | SITES | 20 (v3, port file) |
| 14 | CALLBACK_ORDER | 8, one item, v2 only |
| 15 | SERVICE_IMPORTS | 8, v1 and v2 only |

Version gating: `valid_tag` (keys.rs ~129): v1 tags 1-8, v2 tags 1-11, v3 tags 1-13, plus 14 only in v2 with count 1, plus 15 only in v1/v2. Item sizes by version: `item_bytes_v` (keys.rs:114). Writer picks the version by content (`Table::version`, keys.rs:297): 3 if FWNAMES or SITES, 2 if PROVIDES/BIND/FW/CALLBACK_ORDER, else 1. Constants: `VERSION` (keys.rs:53) from `[limits.format] table_version = 3` (sdk/sdk.toml:2098); `BOOT_TABLE_MAX = 640` (keys.rs:58).

Role rules in the loader (`runtime/core/src/rs/src/loader/role.rs:~495-530`): a module (role 0) may not have service or port tags (E_FORMAT); a service (role 1) must be exactly `keys.version() == 2` with one PROVIDES (line 512); a port file (role 2) must be exactly version 3 (line 523).

### Readers

- **Rust parser, one source for runtime and patchkit:** `Keys::parse` (keys.rs:530) refuses a bad magic or version (`Format`), an unknown or out-of-order tag (`Tag`). Used by the runtime in `loader/{mod,discover,live,role,port,package}.rs`, `lifecycle/{manifest,rt}.rs`, `runtime/core/lifecycle/src/graph/from_tables.rs`, `admission.rs`, `runtime/core/abi/src/keys.rs` (`KeysCopy::from_bytes`, `keys_of`), `abi/service.rs` (`Declare` mirrors tags 4-8). Format maps to `E_CONTRACT`, other errors to `E_FORMAT`.
- **dkneeds menu reader:** `packages/dkneeds/src/lib.rs:212 keys_table` (own entry loop, `valid_tag`, refuses unknown tags with `Error::Register`). It reads only USES and has hard-coded `RES_*` ids (lines ~258-275); it feeds `Needs::fields` (lib.rs:109, 11 fields) and `patchkit.wasm` `pk_needs`.
- **patchkit:** writers `dkm.rs` (`pack_*`, service hooks/slots/exports/kinds at ~806-935, `header_table` pieces at 58-340), `major3.rs:77 header_table` (USES + KEYS for modules), `portfile.rs` (FW, FWNAMES, SITES, reads at 516 and 563), `dkmsynth.rs`, `dkm_identity.rs`. Readers: `services.rs:47`, `main.rs:1057` (inspect), `fit.rs`, `records_cli.rs`, `record_rules.rs`, `svccheck.rs`, `explain.rs`, `module.rs`.
- **Wasm:** `patchkit.wasm` is the Rust code above, so the web app and workbench follow dkneeds automatically (`packages/workbench-core/src/modules/dkneeds.ts`, `fit.ts`, `apps/desktop/ui/src/memory-core.ts`).
- **TypeScript decoders that parse bytes themselves (skip unknown tags, gate only on version <= TABLE_VERSION):**
  - `packages/workbench-core/src/modules/dktransfer.ts:140-200` (`groupOf`, `hasHeaderTable`; reads KEYS groups at an 8-byte stride and maps kinds through `RECORD_KIND`)
  - `packages/workbench-core/src/modules/services.ts:~105-175` (`tableItems`, `readNeeds`, `readProvides`, `readFw`)
  - `tests/regression/runtime/dkm.ts:21` (reads USES resource 4)
  - `apps/web/tests/support/dkm.ts` (hand-builds v1 tables with a `tableVersion` knob)
  - Constants come from `packages/dk-session/src/gen_proto.ts:24` (generated).
- **Python:** `tools/device/dkmodule.py:16-60` (KEYS_TAG, version gate, global kinds), `tools/patchkit/pk_digikit_upload.py`, generated `tools/device/dkproto.py`.
- **Refuse unknown tags:** the Rust parser, dkneeds `keys_table`, the loader's role rules. **Skip unknown tags:** the TS decoders and the Python reader (they only reject a version above their constant).

## 2. Where kinds and capacity ids are global

- `sdk/sdk.toml:2041-2052 [records.kinds]`: PARAM 1, MACHINE 2, PAGE 3, EFFECT 4, ACTION 5, NATIVE_PAGE 6, NATIVE_PAGE_V2 7, LFO_SHAPE 8 (generic), SHORTCUT 9 (each with `len` and owning `service`). `[records.resources]` (2054-2060): MACHINE_ROWS 1, PARAM_ROWS 2, PAGE_ROWS 3, RECORD_BYTES 4, DSP_COST_VOICE 5, DSP_COST_FX 6, each with a `capacity` link. `[capacity.kinds]` (1985-2006): ids 1-16 (MODULES, SELECTION, ARENA, STAGE, DSP_AREA, MACHINE_ROWS, VOICE_ROWS, PARAM_ROWS, PAGE_ROWS, FX_SLOTS, MAP_ENTRIES, STORE, SETTINGS, SEND_SLOTS, DSP_COST, DSP_VOICES), u8 on the CAPACITY wire. `[[service]]` rows (2010-2037) list the in-crate services; the `.dks`-only ones have no `rust`.
- Generated: `packages/dkneeds/src/gen_limits.rs` (`KIND_*` 194-210, `KINDS`/`GENERIC_KINDS` tables 214-235, `RES_*` 270-283, `CAP_*`), `runtime/core/sdk/src/gen_sdk.rs` (`records::KINDS`, `KIND_*`, `CAP_*`), `sdk/include/dk/gen/{records,limits}.h`, `packages/dk-session/src/gen_proto.ts` (`RECORD_KIND`, `TABLE_VERSION`), `packages/dkupload/src/gen_proto.rs`, `tools/device/dkproto.py`; generator `packages/patchkit/src/sdkgen.rs` (22 uses).
- Uses (hit counts of the global ids): `runtime/core/src/rs/src/admission.rs` (44, owner and key rules, `needs_of_uses`, `take_fit`), `runtime/platform/ui/host/src/lib.rs` (21), `patchkit/src/register.rs` (17, DKRG parse by kind), `services/machines/src/{validate,build}.rs` (21), `report.rs` (10) and `admin_cmds.rs` (8, CAPACITY), `loader/mod.rs` (6), `services/{lfo,shortcuts,fx}/src/*.rs`, `patchkit/src/{major3,dkm,fit,record_rules,svccheck}.rs`, `dkneeds/src/lib.rs`, `abi/src/{keys,service,regcheck}.rs`, `handlers.rs`, `collect.rs`, `scan.rs`, `fit/src/{lib,ledger}.rs`.
- The `.dkm` register section is `DKRG` version 1 (`patchkit/src/register.rs:9`): 16-byte head, records `u32 kind, u32 len` (header included); parsed in `runtime/core/abi/src/regcheck.rs`, `register.rs:812`, the `RecordSink::head` in `abi/service.rs:245`.
- The ledger crate exists but is unused (`runtime/core/fit/src/ledger.rs`: `PoolKey(owner, name)`, `Aggregate::{Sum, Max{scale}}`, `ResourceDecl{name, limit, aggregate, draws_from}`). `patchkit/src/service_resource.rs` parses `[[service.resource]]` (limit number, `profile:<key>` or `init`; `aggregate`, `scale`, `draws_from`) but does not pack it ("the packer does not write it yet (RC3)").
- `svc_rows.rs` carries the fallback callback order for the six shipped services (lines 10-20) and a `Rows` table; both go away with the row/part rewrite.

## 3. Proposed version 4

One rule: **v4 is the only version the new packer writes and the new runtime reads.** No version-by-content, no v1/v2/v3 item layouts. The runtime MINOR bump already makes every older file `E_CONTRACT` (see the bump rule below), so v3 compatibility in the on-device parser has no user. This deletes `item_bytes_v` branches, `Table::version` and the SLOTS 8-byte case: a size win, which RC3 needs ("must shrink").

Identities: `S(x) = name_hash(service name)` (FNV-1a, keys.rs:141). The core uses `S("core")`. Tag numbers stay; item layouts are version-defined as SLOTS already is.

(a) **Records keyed (service, local kind)**
- `KINDS` (12 bytes, unchanged shape): local kind, record len, handler entry. The owner is the file's own PROVIDES service, so the "one owner per kind" rule becomes "the service name is unique" plus "no local kind twice in one file".
- `KEYS` group header grows to 12 bytes: u32 service hash, u32 local kind, u32 n, then n keys. A content module's `KEYS` group for machines is `(S("machines"), MACHINE)`, effects `(S("fx"), EFFECT)`, LFO shapes `(S("lfo"), LFO_SHAPE)`. Uniqueness is per `(service, kind, key)` (today per `(kind, key)`).
- The packer still writes the hard NEED on `S(service)` for every group (`dkm.rs:207 record_kind_needs`), so admission orders a module after its service.
- Local kinds are numbered from 1 per service in the service's manifest `[[service.record]]` (name, kind, len; plan section 5). `[records.kinds]` is deleted.
- DKRG becomes version 2. Suggested record header: `u32 service hash, u16 local kind, u16 len` (still 8 bytes, so every record body and offset keeps its size; the largest record is 0x1a4). `RecordSink::head`, `regcheck.rs`, `register.rs`, `collect.rs`, `scan.rs` and the registry walker route by `(service, kind)` to the service's `Records` part.

(b) **Resources per service**
- `RESOURCES` item grows to 24 bytes: u32 name hash; u32 limit value; u8 limit_src (0 number, 1 profile key, 2 init), u8 aggregate (0 sum, 1 max), u8 scale_src (0 number, 1 profile key, 2 none), u8 reserved; u32 scale value; u32 draws_from service hash (0 none); u32 draws_from resource hash. For `profile:<key>` the value is `name_hash(key)`; the runtime resolves it with a small generated table in `abi/src/profiles.rs` (`fx_slots`, `dsp_voices`, `machine_rows`, ...). Service files are device neutral (hooks carry a device mask), so the limit cannot be baked in at pack time.
- Owner is the file's PROVIDES service; the core's resources (`core.modules, arena, stage, dsp_area, dsp_cost, store, settings`) are built into the runtime, not packed. They are the pools `S("core")`, name hash.
- `USES` item grows to 12 bytes: u32 service hash, u32 resource name hash, u32 count. Existing ids map to `machines.machine_rows`, `machines.param_rows`, `machines.page_rows`, `machines.voice_cost` (was DSP_COST_VOICE, max with scale `profile:dsp_voices`, draws_from `core.dsp_cost`), `fx.fx_cost` (was DSP_COST_FX, sum, draws_from `core.dsp_cost`), and `RECORD_BYTES` becomes a claim on `core.arena` (count in bytes; the arena block rounding stays in the claim, `dkneeds::arena_block`).
- Fit is the ledger (`PoolKey`, `Want`) built from tables only: `admission.rs` `needs_of_uses`/`take_fit` and `fit::Needs` are replaced. A `limit = init` pool is checked at `start`.
- CAPACITY reply entry becomes `(owner hash, name hash, limit, used, largest)` and `[capacity.kinds]` goes. This is an upload-protocol MINOR touching `dkupload`, `dk-session`, `dkproto.py`, `memory-core.ts` and the web fit tests; see decision D3.

(c) **Hooks keyed (name hash, device mask)**
- Item layout unchanged (8 bytes: hash, entry with the mask in the top byte). Change the rules: sort by `(hash, mask)`; two items may share a hash only when their masks are non-zero and disjoint; mask 0 (all devices) excludes any other item of that hash.
- Writer: lift the duplicate check at `patchkit/src/dkm.rs:820` and `module.rs` (the "declared twice ... a hook for two devices is one entry without `devices`" rule near line 282) to "twice on overlapping devices"; `svccheck.rs` (manifest equals the `service!` call per device, `hook_set`) and the `service!` macro follow.
- Loader: `role.rs:101 hook_items` currently looks up `find_site(hash)` and discards the mask; it must skip items whose mask lacks the running device (`slot_for_device`-style helper) before the port lookup, and `role.rs:207-210` (`HOOKS` entry masking) and `admission.rs:145-156` (`shared(.., tag::HOOKS)` clash) must compare `(hash, overlapping mask)` instead of hash alone. The port file keeps one `SITES` row per hook hash per firmware, so plain names work: every port file defines `cf.lfo.hook.*` for its own device, and the lfo service carries two HOOKS items per name with different routines. This lets the RC4-lfo `*_dt2`/`*_dn2` names (lane B, not in this commit) return to plain names (`ports/*.toml`, `ports/fw/lfo.toml`, `services/lfo/dks/module.toml`, `image-modules/points/lfo/module.toml`).

(d) **Settings (section 6.6)**
- Service settings stay in code: the `Settings` provider part (`fields`, `choices`, `validate`) is a function table in the `ProviderRow` (C ABI), found by tag at admission. Fields can depend on live state, and the menu only renders loaded services, so the header needs no field schema. No new header tag.
- Module fields (RC3m) are packed as records. v4 gives them an owner key for free: a record `(S("core"), FIELD)` or `(S("settings"), FIELD)` routed to the settings broker. The only header decision is whether the core may own a record kind (D4). The generic record path (`dk_record_add`, `GENERIC_KINDS`) already carries this shape.

### Version and acceptance rules

- `[limits.format] table_version = 4`. Runtime accepts exactly 4: `keys::VERSION` is also the lower bound, and `valid_tag(4, ..)` must allow tags 1-15 (CALLBACK_ORDER and SERVICE_IMPORTS are v2-only today and services need them). Role 1 must check `== 4` (role.rs:512) and role 2 `== 4` (523); the port file is repacked at 4 (its tags are unchanged).
- Reader of v3 files: refuse. Rust returns `KeysError::Format` (`E_CONTRACT`), TS and Python return "built for an older runtime" (`OLDER_RUNTIME_TEXT`). `tableVersionOk` changes from `1..=TABLE_VERSION` to `== TABLE_VERSION`.
- Bump rule from `sdk/sdk.toml:7-19`: a change of the table is a MINOR. `[runtime] version` 0.11.2 to 0.12.0, `table` = "0.12.0" (the least version a packed `.dkm` needs; the loader refuses an older MINOR with `E_CONTRACT`). `service_registration`/`resident_query` floors can stay below or follow to 0.12.0 (they only gate features; pack writes at least `table`). DKRG version 2 and the CAPACITY wire change belong to the same MINOR.
- `BOOT_TABLE_MAX = 640` (keys.rs:58) and `dkm.rs:~950` (a service table longer than this fails the boot capture): wider items grow tables (USES +50%, RESOURCES x3, KEYS group +4). Machines has 13 hooks, 6 kinds and exports; measure with `patchkit size` before fixing the item layout. The cap may need to rise (boot frame is 6,740 B).

## 4. Migration

Repack (all mechanical via Nx pack targets, but every file changes):
- 6 service `.dks`: samples, machines, fx, lfo, shortcuts, project (`services/*/dks/module.toml` gain `[[service.resource]]` and `[[service.record]]`; `services/*/src/{records,validate,build}.rs` move from `KIND_*` globals to local kinds generated from their own manifest; `sdk/templates/service`).
- Content modules: `modules/fx/digikit/freqshift`, `lfo-shape/digikit/lfo_shapes_1`, machines tb303/pd/analog/bd/pluck/shot, tool bootscreen, and the ~50 fixtures in `tests/fixtures/modules` (local-ping*, svc-test*, svc-echo, lfo-shape*, settings-*, dyn-fx-*, ...). Plus the three port files (`ports/{dt2-1.17,dn2-1.11,dn2-1.12}.toml`, role 2).
- Cards in the field: all refused (`E_CONTRACT`) until repacked; the web/workbench library shows "built for an older runtime".

Must change together in one PR (they share `dkneeds` or the format):
1. `dkneeds` `keys.rs` (v4 parse/write, item layouts), `gen_limits.rs` generation, `sdkgen.rs`, `sdk.toml` (version, delete `[records.kinds]`, `[records.resources]`, `[capacity.kinds]` domain ids, keep core ones), `lib.rs keys_table` (the menu reader needs the service and resource hashes; its fields `machine_rows`... become ledger queries or a generic list).
2. patchkit packer/readers: `dkm.rs`, `major3.rs`, `module.rs`, `register.rs` (DKRG 2), `service_resource.rs` (now packed), `records_cli/record_rules/svccheck/fit/explain`, `portfile.rs`, `services.rs`, `main.rs`, synth and identity helpers.
3. Runtime: `abi/{keys,service,regcheck}.rs`, `Declare` (resource items), `collect.rs`, `loader/{mod,discover,role,live,package}.rs`, `admission.rs` (kind owner becomes name uniqueness; fit through the ledger), `lifecycle`, `report.rs`/`admin_cmds.rs` (CAPACITY), `svc_rows.rs`, `handlers.rs`, `fit` crate, `runtime/platform/ui/host` (menu, 21 uses).
4. Decoders, same PR (a v4 file passes their version gate but their group parse is wrong): `dktransfer.ts` `groupOf` (12-byte group header, `(service hash, kind)` pairs instead of `RECORD_KIND`), `services.ts` (if USES or NEEDS read), `tests/regression/runtime/dkm.ts:21` (USES at 12 bytes), `apps/web/tests/support/dkm.ts`, `tools/device/dkmodule.py`, generated `gen_proto.ts`, `dkproto.py`; update `dktransfer.test.ts:81` (versions 1 to TABLE_VERSION) and the `dkneeds.wasm.test.ts` version check. Web and desktop use `patchkit.wasm` for needs, so they change only through its regenerated output (`NeedsLike` in `fit.ts` if the 11 fields change; D3).
5. CAPACITY wire: `dkupload` proto and mock, `dk-session` `dkupload-core.ts`, `apps/desktop/ui/src/{memory-core,memory-backend,profile-backend}.ts`, `apps/desktop/src-tauri` local/main/firmware, `packages/test-support/src/fakedevice.ts`, web tests (`fit.wasm`, `dktransfer`, `samples`).
- Device tests: the 16 files of ledger item RC3, plus the `refusal` and `inferred-needs` tests that patch bytes (`runtime/core/tests/refusal/dkm-patch.ts`, `refusal-table.ts`) because they hard-code kinds and tag layouts.

## 5. Risks and decisions for Em

- **D1 Keep v3 readable or refuse.** Recommend refuse: the MINOR bump refuses all old files anyway; one parser layout is smaller on the ColdFire and removes `Table::version`. Cost: `patchkit inspect` cannot read old files (a clear error). A tool-only v3 reader is possible behind patchkit, not in the runtime.
- **D2 One version for everything vs content-picked.** Recommend always 4 (port file, services, modules): the role checks (`== 2`, `== 3`) become one rule.
- **D3 CAPACITY wire change in this PR or a follow-up.** The v4 header and ledger do not need it, but deleting `[capacity.kinds]` does. Changing it touches the upload protocol, desktop and web UI in one go. Option: ship v4 + ledger with the old CAPACITY ids mapped from core and well-known service pools, then replace the wire as RC3b. Recommend the follow-up to keep the first PR to the format, and say so in the plan row.
- **D4 Can the core own a record kind (settings fields) and a pool owner?** Needed for RC3m records and for `core.*` resources. Recommend yes, with `S("core")` reserved: patchkit refuses a service named `core`.
- **D5 Profile keys by hash.** The runtime needs a generated hash-to-value table for `profile:<key>` limits (a few entries). Alternative: bake the number at pack time per device, which makes service files device specific (breaks the one-file-two-devices rule). Recommend the table.
- **D6 Record header width.** `(hash, u16 kind, u16 len)` keeps record sizes; `(hash, u32 kind, u32 len)` is simpler to read and costs 4 bytes per record and a few offset changes. Recommend the 8-byte form.
- **Risks:** (1) `BOOT_TABLE_MAX` 640 and the boot frame may not hold the wider tables [O: measure with `patchkit size`]. (2) FNV-1a 32-bit collisions between two third-party service names or two resource names in one service: patchkit and admission must refuse; service names already refuse duplicate hashes. (3) `Needs::fields` (11 named fields) is part of the `pk_needs` JSON that the desktop memory pane and web fit tests read; replacing it by a generic list is a UI change. (4) The RC3 row says "must shrink": the `Provider`/`Records`/`Settings` parts and the ledger add runtime text; the v3/v2/v1 branches, `svc_rows` fallback, `plan` leftovers and kind tables are what pays for it [O]. (5) RC4-lfo (lane B, uncommitted) still names hooks `_dt2`/`_dn2` and adds assembly tests that read HOOKS items; land it first or rebase over it. (6) The services' one-per-device `[[hook]]` entries in `module.toml` must match the `service!` call per device (`svccheck.rs:12`); the disjoint-set rule needs the same change in that check or the lfo service fails its own check.

## Decisions (Em, 2026-11-22)

1. Version-3 files are refused; no patchkit-only reader.
2. Patchkit always writes version 4.
3. The CAPACITY wire change is a follow-up PR, not part of RC3.
4. The core may own record kinds and resource pools, under the reserved name "core".
5. Profile-key limits are resolved by hash at run time.
6. The record header is 8 bytes.
