# Workbench module pages

Runtime 0.11.4 exposes public text and file-name settings. Boot screen uses this transport for its active animation. The host preview exercises a generated text form and a colocated Solid companion. The typed settings refactor, module operations and production companion registry below are follow-up work. [Development guide](../DEVELOPMENT.md)

## Discovery and ownership

The selected target owns one inventory of loaded module records. Transfer also lists files on the card. A page lives for one loaded instance and disappears on disconnect, unload or target change. An installed file alone cannot create a page. The sidebar creates one entry per loaded module with public settings, a supported standard panel, or a compatible custom companion.

An author adds `workbench.toml` beside `module.toml`. Patchkit validates the public settings and generates the compact descriptor that the C module registers during init. UI source and labels belong to the module. Importing `dk_settings_open` identifies a storage user for an audit, including calls through linked SDK helpers. Only explicitly declared keys become editable.

The selected target reports each loaded module's canonical author/name ID and, for a registered settings descriptor, its loaded instance token. Workbench describes the instance through the selected session. Transfer works with older runtimes and known file panels remain available for loaded modules. In the future production registry, the host will match a compiled companion by module ID, API version and contract fingerprint. It can still render supported public fields when no companion matches.

## Typed public settings

Keep the module's existing `dk_settings_get`, `dk_settings_put` and `dk_settings_remove` keys and storage. The descriptor tells Workbench how to present and validate each public key. A module with private binary values can expose keys with a compatible encoding or define a separate accessor when it needs one.

| Type | Workbench control | Stored value |
| --- | --- | --- |
| `text` | Text box with a byte limit | UTF-8 text |
| `file-name` | Dropdown of compatible files in the module's data directory | Filename |
| `integer` (proposed) | Numeric input with whole-number and range validation | Decimal text |

Render text and file-name fields in one generated settings form with the same read, save and reset behavior. The file uploader remains a separate action and a successful upload refreshes the available choices. A custom panel should be able to use the module-scoped files and settings clients directly. The control composition can change in Workbench alone.

For integers, extend the packer, runtime validator and Workbench decoder together. Specify the integer range and representation in the schema and enforce both on the runtime. Decimal UTF-8 text fits the existing settings transport. A previously stored binary integer needs an explicit conversion or a different key. Add the new type in a runtime patch release and make Patchkit write the new minimum runtime only for packs that use it. Preserve support for existing text and file-name descriptors. The store, DKM envelope and settings commands can stay as they are.

An older Workbench rejects a descriptor containing an unknown field kind. For the first integer implementation, any descriptor containing an integer requires an updated Workbench to display its settings, including the text or file-name fields in that descriptor. Known file panels can still open.

## Bootscreen

Boot screen declares the animation setting beside its module manifest. Patchkit compiles it into the DKM. The device registers the descriptor when the module loads. Workbench lists choices through the module's existing files client.

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

Uploading an animation leaves the saved choice alone. Save selects a file present in the module's data directory, or removes the setting when Stock is chosen. The runtime uses the same settings namespace as the device menu. With a project settings overlay installed, Workbench can display the effective choice and the device menu handles edits. File contents still pass DKBS validation at boot. If a selected file goes missing, boot falls back to the stock intro.

## Shared module operations

A loaded module can eventually expose named, typed operations beside its settings. The module owns the interface definition, the C handlers and any custom panel. Workbench supplies a client bound to the selected target and loaded instance. The runtime supplies a shared dispatcher and codec, with one copy of the codec on ColdFire. Modules supply operation metadata and handler code. A panel can use the same typed client with an in-memory implementation in preview and tests.

Keep settings and panel metadata in `workbench.toml`. Evaluate a module-local `.proto` for the operation interface. It gives us a standard schema and can generate TypeScript message types with Protobuf-ES and bounded C messages with Nanopb. Digikit still owns the mapping from a stable operation identifier to a C handler, the SysEx transport, and dispatch on the UI task. Prefer generating the mapping from the schema so module authors only implement named functions with generated signatures.

### Project Manager example

The project service owns project identity, storage and consistency. Its current public SDK provides `dk_project_current` and `dk_project_status`. Listing and renaming are future service extensions. The module's handler can delegate to the service once those semantics are defined.

```proto
syntax = "proto3";

package project_manager.v1;

service ProjectManager {
  rpc RenameProject(RenameProjectRequest) returns (RenameProjectResponse);
}

message RenameProjectRequest {
  uint32 slot = 1;
  uint32 revision = 2;
  string name = 3;
}

message RenameProjectResponse {
  uint32 revision = 1;
}
```

The slot and revision form a project reference. The service issues the revision and rechecks it when it applies a rename. Cover slot reuse, concurrent changes, and context changes in the service. Confirm its name encoding and byte limit before defining the production message.

```ts
const renamed = await client.ops.renameProject({
  slot: project.slot,
  revision: project.revision,
  name: draftName,
});
```

The generated C dispatch code would decode the request, invoke the module's `pm_rename_project(ctx, request, response)` on the UI task, and encode the result. The response contains the updated service revision. The panel can then reread the affected page. List operations use bounded pages with a continuation cursor. For an LFO editor, the panel can send the shape asset through the shared files client and call a bounded operation to apply it.

### Codec decision

Keep the codec choice open. A scratch Nanopb 0.4.9.2 probe decoded the request above with a 16-byte name bound and encoded a revision reply. The host round trip accepted a 16-byte name and refused 17 bytes. With the repository's ColdFire compiler targeting MCF54418, the linked codec, descriptors and test handler used 7,742 bytes of code and read-only data and zero bytes of static writable data. The largest compiler-reported frame was 156 bytes. The maximum request and reply were 30 and 6 bytes before SysEx framing. The probe used `PB_BUFFER_ONLY`, `PB_NO_ERRMSG` and `PB_WITHOUT_64BIT`. Patchkit's instruction check passed after compiling with `-fno-jump-tables` to keep jump tables out of executable sections. [Nanopb documentation](https://jpa.kapsi.fi/nanopb/docs/)

Before choosing, compare this with a compact generated binary codec for the same bounded exchange. Measure the shared device code and per-module metadata separately. Use one ColdFire-only execution test to measure instruction counts and peak stack on valid and oversized inputs and confirm both codecs agree with their host clients. The scratch build established cross-compilation and instruction compatibility. ColdFire execution, CPU cost, peak stack and integration into the normal module packer still need verification.

## Compiled Workbench distribution

The source files live with the module for authoring. The release build will compile bundled companions and embed their descriptors in frontend assets. The settings sample registry is a preview entry point only. A production registry will match a loaded module's ID and contract fingerprint to its compiled companion. A module installed after the app release can use standard controls when that Workbench can decode its descriptor. A new executable companion requires a Workbench update in this distribution model. Independent installation of companions and direct Workbench `.dkmod` import are deferred. The existing `.dkmod` authoring export can carry panel source. The device receives the compiled `.dkm` with its device code and descriptor.

The native backend also resolves Workbench Wasm assets through the repository root. Include the required Wasm assets in the release resource lookup. The development library currently calls `lib_build` and scans the repository's `out/modules/<device>/`. Before publishing a standalone catalog, package the prebuilt DKM, DKS and port files with a device-indexed manifest in app resources. Give the released desktop app a library adapter for those resources. The existing repository-backed adapter remains useful for authoring. Verify the published app's offline library and a bundled panel on a machine without the checkout. This packaging work is separate from compiling the frontend.

## Transport and settings semantics

Runtime 0.11.4 advertises `MODULE_UI` for describing a loaded instance and queuing or polling settings work. Operations should extend the module-addressed session with capability negotiation. Version the public operation contract independently of the runtime release. The MIDI handler must return promptly. The UI task rechecks the loaded instance and runs the module handler. Switching targets disposes the client and drops late replies. Unloading cancels queued work before releasing the module's code.

The existing protocol allows 160 request bytes and 879 reply bytes. Budget up to 24 bytes per body for the new envelope, including operation, instance, request ID, revision or cursor. That leaves at least 136 request bytes and 855 reply bytes before encoding individual fields. A 47-byte Bootscreen leaf with a one-byte length fits in 72 bytes including the envelope. Limit a project-list page to 16 summaries of up to 32 encoded bytes each, or 536 bytes including envelope and cursor if the cursor fits the reserved header. Paginate descriptors and lists at complete record boundaries. V1 bounds public scalar/text fields to a single request. Larger assets use the file API. Select the exact field widths after checking one encoded example of each exchange on both device profiles.

MIDI receipt can acknowledge a queued operation with a ticket. Poll for completion through the session with a deadline. A repeated request ID within the same live instance returns its original ticket/result within a bounded retention window. A stale or expired ticket yields an explicit unknown outcome, so clients reread state before offering another mutation. The device reports busy when its bounded queue is full.

The runtime validates public setting types, bounds and owner and checks the edit revision on the UI task before writing. With a project settings overlay installed, Workbench can read the effective settings and edits stay in the device menu until write semantics for that scope are defined. Keep that separate from project operations. A lost operation reply can leave a service mutation committed, so the client should reread current state before retrying with a new request.

## A sample for preview and unit tests

Keep using `tests/fixtures/modules/settings-a/` as the authoring example. It declares a public `boot` text field, and its companion explains when a saved change applies. Its `readAtInit` operation is exercised by the preview fake. It is a candidate for the first real operation fixture once the shared dispatcher exists. Keep its settings proof commands and the settings-b wrapper working.

Build the generic form and this actual panel against a small injected `ModuleClient`. For tests provide `createMemoryModuleClient(descriptor, handlers)` in Workbench test support with a map of values, a revision and module-owned operation handlers. Use the same generated descriptor, panel and registry entry as a real host. Put fixture-specific initial values and the startup-value handler beside the fixture. The memory adapter has no Tauri, Solid, MIDI, filesystem or firmware dependency. Share the pure validation and settings codecs with the real client. Device-side validation remains authoritative. A test can supply a rejecting or deferred operation for error and pending states.

The preview already constructs its sessions per target in `apps/desktop/ui/preview/main.tsx` and derives loaded records from its simulated card in `module-store.ts`. Seed the sample as installed and loaded on the default simulated card. Keep the empty-card fixture empty. Route its instance to the memory adapter through the host's module-client factory. Use the real sidebar discovery, generated form and companion mount. Keep one settings backing map per simulated target, take the startup snapshot on simulated load, and issue a fresh scoped client on reload. Release it on unload or disconnect. Seeding the loaded inventory this way keeps Transfer and the sidebar consistent. The preview should never insert a menu item by hand. Ship fixture registrations only in preview and test builds.

| Test owner | Observable behavior |
| --- | --- |
| Descriptor and form model units | Field validation, empty versus unset, edits and resets, revision conflicts, and a private field missing from discovery. |
| Companion units | Give the real panel a small typed fake. For an interactive companion, verify its result and error states. Control promise completion directly. |
| Target and registry units | A loaded sample gets one page, a card-only sample gets none, a missing companion leaves its standard form, and a late result cannot populate another target. |
| Adapter contract | Run the same short behavior cases against the memory client and the real client over scripted replies. Exercise framing and ticket handling in the protocol adapter tests. |
| Assembly and device | One assembly case for packed metadata and runtime registration. One device round trip for behavior only firmware can establish. Keep the desktop shell, project list and UI permutations out of device tests. |

In the preview, save a boot value, reopen the page and verify the saved value, then reset it. Switch targets and confirm their values remain independent. This scenario also works as a demonstration for module authors.

## Acceptance for the protocol implementation

Use host tests for descriptor generation, matching, codecs and generic form behavior. Cover unknown companions, versions, private fields and target switches. Use runtime and service units for stale instances, ownership, queue limits, duplicate requests and project revisions. The existing settings assembly case loads the real packed settings-a descriptor for both device profiles and checks that the instance becomes unavailable after unload. Add an equivalent assembly case for operation registration when it exists. Keep device proof focused on effects only the running firmware can show, such as applying a saved animation after restart or executing a queued operation on the UI task.
