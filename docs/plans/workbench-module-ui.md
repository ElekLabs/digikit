# Workbench module pages

A loaded module can publish typed settings and a Solid page. Workbench shows the page under Modules in the sidebar while the module is loaded on the selected target. The first real user is a Project Manager module that shows the projects saved on the card. [Development guide](../DEVELOPMENT.md)

Status: runtime 0.11.4 has public `text` and `file-name` settings, used by Boot screen. The sidebar Modules group, the generic form and the preview companion of `tests/fixtures/modules/settings-a` exist. Everything below marked as a phase is open.

## Layers

```text
Module page (Solid, modules/<kind>/<author>/<name>/workbench/)
  | imports only @digi/module-ui, service host packages and solid-js
  v
@digi/module-ui    ModuleClient: typed settings, scoped files, operations (phase 2)
  |                memory client for preview and tests
  v
workbench-core     page registry, generic form, sidebar discovery (desktop and site)
  v
dk-session         MODULE_UI command, files protocol
  v
firmware           module -> service -> runtime
```

- `@digi/module-ui` is the TypeScript contract of a module page, as the C SDK is the contract of module code. A page cannot reach Tauri, dk-session or workbench-core internals. oxlint enforces the import rule.
- `ModuleClient` is message-based: every call is async and takes and returns plain data (structured-clone types). The same page then runs compiled into the app or inside a sandboxed iframe (section "Distribution").
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

The TOML reader in `packages/workbench-core/src/module-ui/descriptor.ts` is deleted. The TS decoder of the binary descriptor stays, because a device reports descriptors. It is checked against shared vectors with the patchkit encoder and the runtime parser.

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

| Item | Expected | Check |
| --- | --- | --- |
| `.dkm` size | Descriptor in read-only data: 4-7 bytes per field plus id and label, about 10 more for an integer, the labels of a choice; at most 512. Getters are inline: no new imports, only used getters cost code. | `.dkm` size of settings-a and Boot screen before and after |
| Runtime code | One-time checks for the new types, about 0.5-1 KB | runtime object size before and after |
| Runtime RAM | No change: the registry already reserves a 512-byte descriptor and a field index per module slot (8 slots) | static size of `REGISTRY` |
| CPU | None while Workbench is idle (the tick returns at an empty queue); one request per UI pass while it talks. Module reads cost the same as today. | none |
| Page code | None on the device | none |

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

- Device side: a SETTINGS row PROJECT with the current project's number, name and status. It calls `dk_project_current` and `dk_project_status`. These imports make the project service a hard need, so Transfer uploads the service with the module and the boot plan loads it.
- Services load only as hard needs of selected modules (`runtime/core/src/rs/src/package/loader/plan.rs` `find_providers`; the system package holds only the runtime and the DIGIKIT menu; Transfer's `serviceRefs` offers a service only for a module that needs it). On a user card the project service runs only while Project Manager, or another module that imports a project call, is loaded. Installing Project Manager turns on per-project settings for every module.
- Workbench page: a list of all project slots by number and name, with a mark on the current project and on slots that carry digikit data. Selecting one shows its saved state (slot, name, save stamp, write counter), its module list with each module's state on the card (present, missing, hash differs), and the number of saved settings per module. The page is read-only: no delete, rename or load.

```text
Modules > Project Manager
+ Projects ------------+ + 012  DUB SKETCH ---------------------+
| 003  INTRO           | | Saved     write 14, stamp 6A1F02C3   |
| 012  DUB SKETCH      | | Modules   digikit/pluck   on card    |
| 047  LIVE SET 2      | |           digikit/shot    missing    |
|                      | | Settings  digikit/pluck   3 keys  >  |
+----------------------+ +--------------------------------------+
```

Data path: page -> `ModuleClient` operation -> module handler -> project service export -> runtime. The `.dkp` format stays private to the service.

- `list(cursor)`: the module calls a new service export over the service's slot scan. A page holds up to 16 summaries (slot, name, flags: current, has digikit data) and a continuation cursor.
- `get(slot)`: the module calls a new service export that returns a bounded summary of one project: header fields, the module list with each module's state on the card, and the number of settings keys per module. It fits one reply; a longer module list pages by cursor.
- Loading and renaming come later through the same path; the service owns their consistency (slot reuse, stamps, concurrent changes).

## Operations (phase 2)

A loaded module can expose named, typed operations beside its settings. The module owns the definitions, the C handlers and its page. Workbench supplies a client bound to the selected target and instance. The runtime supplies one dispatcher and one codec on the ColdFire. Modules supply operation metadata and handlers.

- Schema: operations are declared in `workbench.toml` with the setting types plus bounded lists of records. Patchkit generates the C request and response structs, the dispatch table and handler signatures, and the TypeScript types. The author implements named functions, for example `pm_list(ctx, request, response)`.
- Codec: a tag-length-value format with field ids, generated as `sdk/sdk.toml` is today (`packages/patchkit/src/sdkgen.rs`), with shared vectors. Field ids give the same schema evolution as protobuf. Nanopb is not used for settings. It is the fallback for operations only if a measurement favours it: a scratch probe linked 7,742 bytes of codec, descriptors and handler per module, and it adds a third-party C codec, a Python generator and a host protobuf library.
- Transport: the operation envelope extends the MODULE_UI command with capability negotiation and is versioned apart from the runtime release. The MIDI handler acknowledges with a ticket; the UI task rechecks the instance and runs the handler; the client polls with a deadline. A repeated request id within one live instance returns its original result within a bounded window. A stale ticket yields an explicit unknown outcome, and the client rereads state before another mutation. The device reports busy when its queue is full. Unloading cancels queued work before the module's code is released.
- Budget: the protocol allows 160 request bytes and 879 reply bytes. Reserve up to 24 bytes for the envelope (operation, instance, request id, revision or cursor). A project-list page is at most 16 summaries of up to 32 bytes. Larger data uses the files API.

## Distribution

- In a checkout, a page lives in the module folder. The page registry is generated by an Nx target from the module roots (`modules/`, `tests/fixtures/modules/`, `PATCHKIT_MODULES`) with lazy imports and declared inputs. In the Vite dev server a new or imported module folder appears under Modules after the registry refreshes.
- The released desktop app and the site compile the in-tree pages into their bundles. A module installed after the release gets the generic form when that Workbench can decode its descriptor.
- `.dkmod` import in Workbench (later): import extracts the module, builds its `.dkm` and pushes it with its services; the page stays local and is never sent to the device. In a checkout the extracted page joins the registry. In the released app the page is compiled once at import and runs in a sandboxed iframe through the message-based `ModuleClient`, because code from outside the app build must not get Tauri IPC (the CSP allows scripts from `'self'` only). The page source is the same in both cases.
- The released app needs a library adapter for prebuilt DKM, DKS and port files in app resources; the repository-backed library stays for authoring. This packaging work is separate.

## Preview and tests

`tests/fixtures/modules/settings-a/` stays the authoring example. Phase 1 adds one setting of each new type to it, or a second fixture if a distinct module identity is needed for a version 2 descriptor.

- The preview seeds the sample as installed and loaded on the default simulated card, routes its instance to the memory client and uses the real sidebar discovery, generated form and page. One settings map per simulated target; a fresh client on reload; released on unload or disconnect. The preview never inserts a menu item by hand.
- `createMemoryModuleClient(schema, handlers)` lives in `@digi/module-ui` test support: values, a revision and module-owned operation handlers, with no Tauri, MIDI, filesystem or firmware dependency. It shares the pure validation code with the real client.

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
2. Operations (about 2-3 days, medium confidence): envelope and dispatch on the UI task, generated codec and handler signatures, typed `ops` in `ModuleClient`, memory client handlers, one operation on the settings-a fixture.
3. Project Manager, read-only (about 1-1.5 days): module with the SETTINGS row; service exports for list and get; `list` and `get` operations; the page.

Later: `.dkmod` import in Workbench with the iframe sandbox; a generic device-side settings page under DIGIKIT driven by the same descriptor.
