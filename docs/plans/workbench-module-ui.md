# Workbench module pages

A loaded module can publish typed settings and a Solid page. Workbench shows the page under Modules in the sidebar while the module is loaded on the selected target. The first real user is a Project Manager module that shows the projects saved on the card. [Development guide](../DEVELOPMENT.md)

Status: runtime 0.11.4 has public `text` and `file-name` settings, used by Boot screen. The sidebar Modules group, the generic form (a control for every kind), the page registry (`workbench/index.tsx` with `definePage`), the channel factory (device and memory), Transfer Open and the page import rule exist; `tests/fixtures/modules/settings-a` is the first page. Everything below marked as a phase is open.

## Layers

```text
Module page (Solid, modules/<kind>/<author>/<name>/workbench/)
  | imports only @digi/module-ui, service host packages and solid-js
  v
@digi/module-ui    ModuleClient: typed settings, scoped files, queries (phase 2)
  |                memory client for preview and tests
  v
workbench-core     page registry, generic form, sidebar discovery (desktop and site)
  v
dk-session         MODULE_UI command, files protocol
  v
firmware           module -> service -> runtime
```

- `@digi/module-ui` is the TypeScript contract of a module page, as the C SDK is the contract of module code. A page cannot reach Tauri, dk-session or workbench-core internals. oxlint enforces the import rule.
- `ModuleChannel` is the boundary: the only interface that crosses to a sandboxed page. Every request is async and takes and returns structured-clone data; `callQuery(method, request)` carries one encoded query and returns a `QueryResult` (reply bytes or a `QueryError`: `busy`, `refused` with the handler's code, `unavailable`, `too-big`, `format`, `unknown-method`, `timeout`); progress and other notifications are `ModuleChannelEvent` messages through `subscribe`. The same page then runs compiled into the app or inside a sandboxed iframe (section "Distribution"). The real channel is workbench-core over dk-session; the memory channel serves preview and tests.
- `ModuleClient` (`createModuleClient(channel)`) is a page-side helper: typed settings (validation, scaling, revisions), the files helper with its `progress` callback and `service(desc)`, one async function per method of a protobuf-es service over `callQuery`. It never crosses the boundary.
- A page reaches device data only through its module: settings, the module's own files and the module's operations. A service's files and formats stay private to the service; the module asks the service through its SDK exports.

## Discovery

- The selected target owns one inventory of loaded module records. A page lives for one loaded instance and disappears on disconnect, unload or target change. An installed file alone creates no page.
- The sidebar shows one entry per loaded module that has public settings, a known file panel or a registered page. A module without a matching page gets the generic form of its settings.
- In Transfer, a loaded row that has a page gets an Open action. It navigates to the sidebar page. There is one page view.

## Typed public settings

The author declares settings in `workbench.toml` beside `module.toml`. Patchkit is the only parser of this file. From it, patchkit generates:

1. the compact descriptor that the module registers in init (`dk_workbench_register`),
2. a C header with keys, bounds and typed inline getters over `dk_settings_get`,
3. a TypeScript file with the settings type and schema for the page.

The TOML reader in `packages/workbench-core/src/module-ui/descriptor.ts` is deleted. The TS decoder of the binary descriptor stays, because a device reports descriptors. It is checked against `expected_schema` in the shared vectors (`runtime/core/vectors/workbench-vectors.json`), which patchkit writes from the same declaration as the bytes (`UPDATE_VECTORS=1`); the patchkit encoder and the runtime parser check the bytes against the same file.

| Type | Workbench control | Stored value | Schema fields |
| --- | --- | --- | --- |
| `text` | Text box | UTF-8 | `max_bytes` (1-64) |
| `file-name` | Dropdown of files in the module's data directory | File name | `extension`, `max_bytes` (1-47), `reset_label` |
| `integer` | Number input | i32, big-endian, 4 bytes | `min`, `max`, `step`, `decimals` (0-4), `unit` |
| `boolean` | Toggle | u8, 0 or 1 | none |
| `choice` | Dropdown of labels | u8 index | `choices` (1-32 labels) |

- All settings also take `id`, `label` and `apply` (`immediate` or `next-restart`).
- The ColdFire has no FPU, so there is no IEEE float type. `decimals` scales an integer for display: `decimals = 2` with `min = 0`, `max = 100` shows 0.00 to 1.00, and the module reads 0 to 100. A float type is added only when a module needs one.
- The runtime checks every write on the UI task: length, type, range, step, choice index, UTF-8 and file names. It reads bounds from the registered descriptor bytes, so the registry's static RAM does not grow.
- The store, DKM envelope and MODULE_UI verbs stay. `[workbench] version = 1` is the TOML format and does not change. The first byte of the binary descriptor stays 1 when a module uses only `text` and `file-name`, and is 2 when it uses a new type. Patchkit writes the new minimum runtime only for packs that use a new type. An older Workbench shows the known file panels of a version 2 module but not its settings form.

Example:

```toml
[workbench]
version = 1

[[setting]]
id = "gain"
label = "Gain"
type = "integer"
min = 0
max = 100
decimals = 2
unit = "dB"
apply = "immediate"

[[setting]]
id = "mode"
label = "Mode"
type = "choice"
choices = ["Off", "Soft", "Hard"]
apply = "immediate"
```

Generated TypeScript and its use in a page:

```ts
export interface Settings { gain: number; mode: 'Off' | 'Soft' | 'Hard' }
export const settings: SettingsSchema<Settings> = { /* ids, bounds, labels */ };

const s = client.settings(settings);
await s.put('gain', 0.75);       // checked against min, max, step and decimals before sending
const mode = await s.get('mode');
```

Generated C:

```c
static inline int32_t settings_gain(dk_settings *s);   /* scaled: 75 = 0.75 */
static inline uint8_t settings_mode(dk_settings *s);   /* SETTINGS_MODE_SOFT, ... */
```

### Cost

Measured on the merge of origin/main (e154c975a) with this branch; `patchkit pack` for both devices (the sizes are equal on DT2 and DN2).

| Item | Measured | Check |
| --- | --- | --- |
| `.dkm` size | settings-a 1268 -> 1520 bytes (image 564 -> 736, reloc 320 -> 360, import 192 -> 224, bss 136 unchanged). Boot screen 6020 -> 6020 bytes (no change: it stays a version 1 descriptor). Under the limit of 512 for the descriptor. | `.dkm` size of settings-a and Boot screen before and after |
| Runtime code | Package `.text` 196424 -> 198420 bytes (+1996, about 2 KB; above the 0.5-1 KB estimate: three new types with their checks, the cursor and the getter job) | runtime object size before and after |
| Runtime RAM | No change: `size_of::<Registry>()` is 16656 and `size_of::<Descriptor>()` 1092 on both sides (pinned by `typed_kinds_do_not_grow_the_registry`) | static size of `REGISTRY` |
| CPU | None while Workbench is idle (the tick returns at an empty queue); one request per UI pass while it talks. Module reads cost the same as today. | none |
| Page code | None on the device | none |

settings-a grew by its three new settings and the settings getter call (one more import).

## Boot screen

Boot screen keeps its `file-name` setting and stays a version 1 descriptor.

```toml
[workbench]
version = 1

[[setting]]
id = "active"
label = "Animation"
type = "file-name"
extension = "dkbs"
reset_label = "Stock"
max_bytes = 47
apply = "next-restart"
```

Uploading an animation leaves the saved choice alone. Save selects a file present in the module's data directory, or removes the setting when Stock is chosen. File contents still pass DKBS validation at boot. If a selected file goes missing, boot falls back to the stock intro.

## Project Manager

`modules/tool/digikit/project-manager` is a content module of kind `tool`, like track-manager.

- Device side: a SETTINGS row PROJECT with the current project's number, name and status. Screen text (read-only, `[NO]` closes; the screen reads the service each frame, so it follows a save):
  - `NNN NAME`: slot + 1 in three digits, the firmware's name bytes without trailing spaces; `NO PROJECT` when there is no current project (then no other line).
  - Line 2: `FILE IGNORED` (status bit 3), else `CARD BUSY - RETRYING` (bit 2), else `CHANGED - NOT SAVED` (bit 1), else `SAVED`.
  - Line 4: `NO DIGIKIT DATA` when bit 0 (file loaded) is clear.
- Services load only as hard needs of selected modules (`runtime/core/src/rs/src/package/loader/plan.rs` `find_providers`; the system package holds only the runtime and the DIGIKIT menu; Transfer's `serviceRefs` offers a service only for a module that needs it). On a user card the project service runs only while Project Manager, or another module that imports a project call, is loaded. Installing Project Manager turns on per-project settings for every module.
- Workbench page (`workbench/index.tsx`, built): left a list of the device's projects by number (slot + 1) and name, 16 per page with "More projects" (the reply's `next`), the current project marked, "no digikit data" for slots without data and "…" while that is unknown; right the selected project's number, name, write counter, save stamp and its modules with id, loaded or not, and settings count. For a project other than the current one the device reads its file first: `Get` answers `loading` and the page shows "Loading…" and asks again every 200 ms until it does not, stopping when the selection changes or the page closes. A failed query shows its reason in plain words. The page is read-only: no delete, rename or load.

Data path (built): page -> `client.service(ProjectManager)` -> `workbench/project_manager.proto` (`List`, `Get`) -> C handlers in `src/c/project_queries.c` -> `dk_project_list` / `dk_project_get` of the project service. `DK_E_AGAIN` becomes `loading`, `DK_E_NOT_FOUND` becomes `found = false`; the mapping is in `project_query.c` (host tests in `tests/`). The preview answers from `workbench/preview.ts` (fake projects; INTRO answers `loading` twice). The `.dkp` format stays private to the service. Page tests: `modules/tool/digikit/project-manager/tests/project-manager.page.test.ts` (memory channel). No device test of its own: the fixtures cover the query path.

A module with a page and no settings registers a zero-settings descriptor, its Workbench registration; Project Manager does, and the `test/project-user` fixture covers that contract.

Saved settings per module (built): click a module row on the details pane to expand its saved settings under it.
- Path: page -> `getSettings({ slot, module, version, cursor })` -> Project Manager handler (maps structs to protobuf only, `project_query.c`) -> project service export `dk_project_settings(slot, module_id, id_len, version, cursor, out, max, next)`, which owns the `.dkp` reading and paging. Runtime and transport unchanged. The export is `since` 0.13.5 like `dk_project_get`.
- The request names the module by id and the project's write counter (`version`, the `counter` of `Get`). A different counter answers `DK_E_CHANGED`; the reply has `changed` and the page reads the project again. `DK_E_AGAIN` is `loading`, `DK_E_NOT_FOUND` is `found = false` (no file, or the module is not in the project's module list).
- Each entry (`dk_project_setting`, 116 bytes): the key without the module's prefix, the full value length, the first 64 value bytes; 4 entries per reply and a cursor (the entry index; `next` is 0 after the last entry).
- The open project answers from its live image. Another project keeps one page of up to 4 entries next to its summary: a call reads the `.dkp` (`DK_E_AGAIN` until read), a call for another module or cursor reads it again. The page is the only extra RAM (4 x 116 bytes).
- The page shows a value as text when every shown byte is printable ASCII, else as hex, and "N bytes" when the value is longer than shown. A module with 0 settings is not asked for. Typed values need each module's schema and are later.
- Tests: service units (`services/project/tests/service/settings.rs`: paging, version mismatch, truncation, open vs other project, not found), handler mapping units (`tests/logic-driver.c`), page units (`tests/project-manager.page.test.ts`: expand, paging, loading, text/hex, changed, failures; `tests/project-text.test.ts`); `test/project-user` has the same `GetSettings` query and device case G checks the value it saved.

Raw view and save (built; runtime 0.13.6, which also carries `dk_project_settings`):
- `ReadFile({ slot, version, offset })` -> `dk_project_read(slot, version, offset, out, max, *total)` returns the raw `.dkp` bytes from `offset` (768 per reply) and the length of the whole file. The open project answers from the live image the service holds (its header counter and CRC are those of the last write, so unsaved changes show a CRC that does not match); another slot is read in the background like `Get` (`loading`, `changed` for another counter, `found = false` without a file).
- The page's "Raw" tab on the selected project fetches all pages and shows hex with byte offsets, coloured by section (`workbench/dkp-spans.ts`: header fields, each tag section, module ids and hashes, owners, keys and values; a file of another version or without the magic shows as plain hex). The highlighter is tested against `runtime/core/vectors/dkp-vectors.json`.
- A checkbox per project row and "Save selected": each ticked project's file is read with `ReadFile` and all go to `client.saveFiles` as `NNN-NAME.dkp` (characters a file name cannot hold become `_`); a project that cannot be read is named and left out.

- Loading and renaming come later through the same path; the service owns their consistency (slot reuse, stamps, concurrent changes).

## Queries (phase 2)

A loaded module can answer typed, read-only queries from its page: live state, lists, details. Actions that change state come later as their own step with retry safety (retained results, repeat request ids, an explicit unknown outcome).

- **Schema:** a module-local `workbench/<name>.proto` holding one `service` of read-only methods. A subset of proto3 only: `uint32`, `int32`, `bool`, enums, `string`/`bytes` with `[(dk.max_len)]`, nested messages, `repeated` with `[(dk.max_count)]` (`sdk/proto/dk/options.proto`). Patchkit refuses maps, `oneof`, floats, 64-bit fields, groups and unbounded fields.
- **Code generation, at build time, in patchkit (Rust, `protox`):** per message a C struct with straight-line `_decode` and `_encode` functions and a maximum encoded size; per method a call function (decode request, call the handler, encode reply) and the handler prototype the author implements, e.g. `int32_t pm_list(dk_module_ctx *, const pm_ListRequest *, pm_ListReply *)`; the query table. Decoders skip unknown fields. Patchkit refuses a method whose request or reply can exceed the transport budget, and a struct over the stack budget. No tables are interpreted on the device; nothing from nanopb or Python.
- **Wire helpers:** varint, key, length and skip helpers (`sdk/include/dk/wire.h`, `sdk/src/wire/wire.c`) are linked into each module that uses queries (about 600 bytes). The runtime may provide them once as SDK imports later, when several modules use queries.
- **TypeScript:** `protobuf-es` from the same `.proto`. Patchkit writes the descriptor set; `packages/wire` feeds it to `protoc-gen-es` (no `protoc` or `buf`). Pages call `client.service(ProjectManager).list(...)`.
- **Binary interface:** in `init` the module calls `dk_workbench_register_queries(ctx, const dk_workbench_query *table, uint32_t count)` after `dk_workbench_register`. `dk_workbench_query` (`sdk/include/dk/query.h`, the one definition) is `{ int32_t (*call)(dk_module_ctx *, const uint8_t *in, uint32_t in_len, uint8_t *out, uint32_t *out_len); uint8_t method; uint8_t reserved[3]; }`: 8 bytes in the module and in the runtime (`#[repr(C)]`, asserted on both sides). The layout is explicit because the ColdFire compiler aligns pointers to 2 bytes (no `-malign-int`), so a table may start on any 2-byte boundary and the runtime reads entries unaligned. `method` is the method's 1-based position in the `service`; methods are only appended, and patchkit checks that against the previous descriptor. The runtime treats requests and replies as bytes.
- **Transport:** the MODULE_UI command gains a call verb: `[verb][rec][instance u32][request id u32][method u8][request bytes]`. The limits are in `sdk/sdk.toml` `[protocol.limits]` (`query_request_max` 149 request bytes after the envelope, `query_reply_max` 870 reply bytes after the poll header); the runtime and patchkit's budget check read the same values. The MIDI handler checks the envelope and queues; on the next UI pass the runtime checks the loaded instance, calls the module's function with an output buffer of the reply budget, and keeps the outcome for the poll verb. The poll reply of a call is `[state u8][runtime status i32][handler result i32][reply bytes]`: the runtime status is OK when the handler ran, the handler's return value is only in its own field. One call is in flight for the whole device (one request buffer, one outcome buffer): a new call from any module answers busy while a call is queued, running or its outcome is unread. The poll hands the outcome over and frees it; an unread outcome is freed after 2 s. The settings request and the call are served in turn, one per UI pass. A call is matched by module, instance and request id, so a reload into the same slot cannot take the old call's result; unloading runs on the UI task, never inside a call, and cancels a queued call or held outcome before the code is released. Capability bit `MODULE_QUERIES`; patchkit writes the new minimum runtime only for modules that declare queries.
- **Client results:** `{ ok: true, value }` or `{ ok: false, error }` with `busy`, `refused` (the handler's error code), `unavailable`, `too-big`, `timeout`. A read is safe to repeat, so a lost reply is asked again.
- **Tests:** codec round trips against `protobuf-es` (`runtime/core/vectors/wire-vectors.json`) on the host; patchkit schema and budget units; runtime queue units; `@digi/module-ui` client units with the memory channel; one assembly case (the packed fixture registers its table and one call round-trips); one device test per device (a call reaches the handler on the UI task).

## Distribution

- In a checkout, a page lives in the module folder. The page registry is generated by an Nx target from the module roots (`modules/`, `tests/fixtures/modules/`, `PATCHKIT_MODULES`) with lazy imports and declared inputs. In the Vite dev server a new or imported module folder appears under Modules after the registry refreshes.
- The released desktop app and the site compile the in-tree pages into their bundles. A module installed after the release gets the generic form when that Workbench can decode its descriptor.
- `.dkmod` import in Workbench (later): import extracts the module, builds its `.dkm` and pushes it with its services; the page stays local and is never sent to the device. In a checkout the extracted page joins the registry. In the released app the page is compiled once at import and runs in a sandboxed iframe through the message-based `ModuleClient`, because code from outside the app build must not get Tauri IPC (the CSP allows scripts from `'self'` only). The page source is the same in both cases.
- The released app needs a library adapter for prebuilt DKM, DKS and port files in app resources; the repository-backed library stays for authoring. This packaging work is separate.

## Preview and tests

`tests/fixtures/modules/settings-a/` stays the authoring example. Phase 1 adds one setting of each new type to it, or a second fixture if a distinct module identity is needed for a version 2 descriptor.

- The preview seeds the sample as installed and loaded on the default simulated card, routes its instance to the memory client and uses the real sidebar discovery, generated form and page. One settings store per simulated target, kept for the target's life (device settings persist across module reloads); a fresh client and channel on each page open; channels are released on unload, disconnect or target change. The preview never inserts a menu item by hand.
- `createMemoryModuleChannel(schema, options)` lives in `@digi/module-ui`: a `ModuleChannel` with values, a revision and files and one query service (`memoryQueryService(desc, handlers)`; a handler throws `QueryRefusal(code)` to refuse; `inspect.failNextQuery` forces `busy` or `timeout`), with no Tauri, MIDI, filesystem or firmware dependency. It shares the pure validation code with the real client.

| Test owner | Observable behaviour |
| --- | --- |
| Patchkit units | Each type's schema checks and encoding; version 1 kept for text and file-name only; shared vectors. |
| Runtime units | Each type's write check (range, step, choice index, length); revision conflict; version 2 parse. |
| `@digi/module-ui` units | Typed client: value scaling, client-side checks, empty versus unset. |
| Workbench units | A loaded sample gets one page, a card-only sample none, a missing page leaves the generic form, a late result cannot reach another target, Transfer Open navigates. |
| Project service units | List and get summaries from the slot scan and the bound image: paging, current mark, a slot without digikit data, a module list longer than one reply. |
| Assembly | The packed version 2 settings fixture registers on both device profiles. |
| Device | One round trip per new behaviour only firmware shows: a typed put and get on DN2 and DT2. Project Manager: the SETTINGS row and the service loaded as its need. |

## Working rules

- Test pyramid: unit tests with fakes, stubs and memory clients for logic; assembly tests for seams between built artifacts; device tests only for what firmware shows. Each assertion sits at the lowest layer that can see it.
- Structure: one concept per file, named after its noun; methods are verbs; state is private and changes through the owner's methods; lifecycle (open, reload, release) belongs to one owner. Each rule lives once (DRY); shared vectors tie the Rust, C and TypeScript copies of a wire format together.
- Every step: one worker with a self-contained brief and the first-pass checklist, then an independent review, then one fix round.

## Phases

1. Typed settings and page plumbing (about 1-1.5 days): new types in patchkit, runtime and TS; generated C and TS; `@digi/module-ui` with the memory client; generated page registry; Transfer Open; settings-a migrated; descriptor TOML reader deleted; cost table measured.
2. Queries (about 1.5-2 days): generated codecs and call functions, runtime call verb and queue, `client.service()` and memory-channel handlers, one query on the settings-a fixture.
3. Project Manager, read-only (about 1-1.5 days): module with the SETTINGS row; service exports for list and get; `list` and `get` operations; the page.

Later: `.dkmod` import in Workbench with the iframe sandbox; a generic device-side settings page under DIGIKIT driven by the same descriptor.
