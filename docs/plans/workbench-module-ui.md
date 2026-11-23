# Workbench module pages

Proposal for review. The offline Transfer change is independent. The examples below describe proposed authoring and transport APIs.

## Discovery and ownership

The selected target owns one inventory of loaded module records. Transfer also lists files on the card. A page lives for one loaded instance and disappears on disconnect, unload or target change. An installed file alone cannot create a page. The sidebar creates one entry per loaded module with public settings, a supported standard panel, or a compatible custom companion.

An author adds `workbench.toml` beside `module.toml`. Patchkit reads the optional file during the module build and validates unique public names, bounds and codecs. Its outputs are a compact device descriptor, C binding declarations and TypeScript client types from the same definition. UI source and labels belong to the module. Importing `dk_settings_open` identifies a storage user for an audit, including calls through linked SDK helpers. Only explicit public fields become editable. Private keys, binary layouts and unrelated SDK imports stay internal.

The loaded instance advertises its canonical author/name ID, descriptor format version and contract fingerprint. The host matches a compiled companion by that tuple. The instance handle additionally carries a boot/load generation so returning to the same target or reusing a record slot cannot revive an old handle. On older firmware, keep Transfer available and omit module pages. For an unknown companion, render any supported public fields and standard panels advertised by the device. Treat unsupported descriptor versions as unavailable.

## Bootscreen

The author edits `modules/tool/tools/bootscreen/workbench.toml` and the module's C source only where validation needs module behavior. Move the existing file presentation metadata into the new declaration and generate the file panel from it. Keep one source of labels, limits and preview choice.

```toml
[workbench]
version = 1

[[panel]]
kind = "files"
id = "animations"
label = "Boot screens"
extensions = ["dkbs"]
preview = "oled-1bit"

[[setting]]
id = "active"
label = "Boot screen"
type = "file-name"
files = "animations"
max_bytes = 47
binding = "active"
reset_label = "Stock"
apply = "next-restart"
```

The host generates the form and the file controls. The settings client exposes `list()`, `get(key)`, `put(key, value, revision)`, and `reset(key, revision)` for resettable fields. The files client offers list, read, put with progress, and remove inside the loaded module's data directory. Send uses the existing file transport. It leaves the selection up to the user. The active chooser lists uploaded files and Stock. Selecting Stock removes the key. An absent key and a stored empty value remain distinguishable.

1. Discover the loaded instance and read its descriptor in pages. Read `active` with its revision and enumerate the module's files.
2. Upload the chosen DKBS through the existing file API. Keep the 64 KiB file limit and existing host preview validation. Refresh the file list after completion.
3. Send a typed write with the instance, field ID, observed revision and file name. The UI task checks the binding, length, leaf name and membership in the module's folder, then uses the module's settings handle. Reuse the device chooser's validation path. File contents still pass the module's full DKBS validation at boot.
4. Return the effective value, new revision and next-restart status. Reset follows the same ownership checks. A file deleted after selection follows the existing stock-intro fallback.

## Project Manager

The future module adds `workbench.toml`, `workbench/Panel.tsx` and its C operation handlers in its own folder. The project service owns project identity, storage and consistency. Its current public SDK provides `dk_project_current` and `dk_project_status`. Listing and renaming are proposed service extensions. They need service-level semantics and tests before their handlers ship.

```toml
[workbench]
version = 1
component = "workbench/Panel.tsx"

[[operation]]
id = "listProjects"
input = { cursor = "u16" }
output = { projects = "ProjectSummary[]", next = "optional<u16>" }

[[operation]]
id = "renameProject"
input = { project = "ProjectRef", name = "text<16>" }
output = { project = "ProjectSummary" }
```

Here `ProjectRef` and `ProjectSummary` denote module-declared record types. A reference includes a slot and an opaque revision issued by the service. Service-side stale checks cover slot reuse, concurrent changes and switches of current project. Confirm the supported encoding and length with the service when designing rename.

```tsx
export default function Panel({ client }: ProjectManagerPanelProps) {
  const [page, { refetch }] = createResource(() => client.ops.listProjects({ cursor: 0 }));
  async function rename(project: ProjectRef, name: string) {
    await client.ops.renameProject({ project, name });
    await refetch();
  }
  return <ProjectList page={page()} onRename={rename} />;
}
```

`ProjectList` is module-owned. The host mounts the component beside any generated settings form and supplies the client bound to that page instance. An author can use Solid for pagination, progress or canvas interaction. A generated static registry bundles compatible companions into desktop and web. Installing a DKM can add standard UI immediately. Installing executable third-party companions independently would require a separate packaging and isolation design.

A list request returns a bounded page with a continuation cursor. Rename submits a bounded request. The dispatcher queues it for the UI task, which rechecks the loaded owner, validates the project reference, and calls the service. The client resolves once the operation completes, then the panel rereads the list. Switching target disposes the client and drops late replies. Unloading cancels queued work before module code is released. An already committed service mutation remains committed if its reply is lost.

## Transport and settings semantics

Negotiate a runtime capability for module UI v1. Use one module-addressed command family over the existing session. Proposed verbs cover discovery, descriptor pages, reading and updating public settings, invoking named operations and obtaining a queued result. The module command handler runs on the MIDI task and must return promptly. The UI task executes storage access and module callbacks. Version the public descriptor independently of the runtime release.

The existing protocol allows 160 request bytes and 879 reply bytes. Budget up to 24 bytes per body for the new envelope, including operation, instance, request ID, revision or cursor. That leaves at least 136 request bytes and 855 reply bytes before encoding individual fields. A 47-byte Bootscreen leaf with a one-byte length fits in 72 bytes including the envelope. Limit a project-list page to 16 summaries of up to 32 encoded bytes each, or 536 bytes including envelope and cursor if the cursor fits the reserved header. Paginate descriptors and lists at complete record boundaries. V1 bounds public scalar/text fields to a single request. Larger assets use the file API. Select the exact field widths after checking one encoded example of each exchange on both device profiles.

MIDI receipt can acknowledge a queued operation with a ticket. Poll for completion through the session with a deadline. A repeated request ID within the same live instance returns its original ticket/result within a bounded retention window. A stale or expired ticket yields an explicit unknown outcome, so clients reread state before offering another mutation. The device reports busy when its bounded queue is full.

The runtime validates every field's type, bounds, access mode, schema and owner. Simple scalar bindings use the existing settings overlay, including its fallback and normal save timing. An accessor binding can validate or update a field in a packed record. The device issues an edit revision that changes on an effective value change or a project-context change, including an A–B–A switch. Recheck that revision on the UI task immediately before writing. Return a conflict and the current value on mismatch. The module declares whether a change applies immediately, through a reload or on the next restart. Return pending/rejected reload outcomes to the client when relevant. The host never opens the global settings record.

## A sample for preview and unit tests

Use the existing `tests/fixtures/modules/settings-a/` module as the authoring example. Give it a `workbench.toml` declaring a public `boot` text field (up to 16 bytes) and a `readAtInit` operation. Its `workbench/Panel.tsx` shows the value the module read during init when the user clicks **Read at startup**. This makes the distinction visible: editing the current setting updates the standard form immediately, while the custom panel reports the old startup value until the next simulated load. The existing C fixture already captures that value. Keep its current proof commands and the settings-b wrapper working.

Build the generic form and this actual panel against a small injected `ModuleClient`. For tests provide `createMemoryModuleClient(descriptor, handlers)` in Workbench test support with a map of values, a revision and module-owned operation handlers. Use the same generated descriptor, panel and registry entry as a real host. Put fixture-specific initial values and the startup-value handler beside the fixture. The memory adapter has no Tauri, Solid, MIDI, filesystem or firmware dependency. Share the pure validation and settings codecs with the real client. Device-side validation remains authoritative. A test can supply a rejecting or deferred operation for error and pending states.

The preview already constructs its sessions per target in `apps/desktop/ui/preview/main.tsx` and derives loaded records from its simulated card in `module-store.ts`. Add an opt-in preview scenario that seeds the sample as installed and loaded on that card. Route its instance to the memory adapter through the host's module-client factory. Use the real sidebar discovery, generated form and companion mount. Keep one settings backing map per simulated target, take the startup snapshot on simulated load, and issue a fresh scoped client on reload. Release it on unload or disconnect. Seeding the loaded inventory this way keeps Transfer and the sidebar consistent. The preview should never insert a menu item by hand. Ship fixture registrations only in preview and test builds.

| Test owner | Observable behavior |
| --- | --- |
| Descriptor and form model units | Field validation, empty versus unset, edits and resets, revision conflicts, and a private field missing from discovery. |
| Companion units | Give the real panel a small typed fake. Verify it displays the returned startup value and recovers from an operation error. Control promise completion directly. |
| Target and registry units | A loaded sample gets one page, a card-only sample gets none, a missing companion leaves its standard form, and a late result cannot populate another target. |
| Adapter contract | Run the same short behavior cases against the memory client and the real client over scripted replies. Exercise framing and ticket handling in the protocol adapter tests. |
| Assembly and device | One assembly case for packed metadata and runtime registration. One device round trip for behavior only firmware can establish. Keep the desktop shell, project list and UI permutations out of device tests. |

Preview acceptance is a short manual interaction: edit the field, read the startup snapshot through the custom panel, then reload and see the new snapshot. Switch targets and confirm their values remain independent. This scenario also works as a demonstration for module authors.

## Acceptance for the protocol implementation

Use host tests for descriptor generation, matching, codecs and generic form behavior. Cover unknown companions, versions, private fields and target switches. Use runtime and service units for stale instances, ownership, queue limits, duplicate requests, project revisions and reload disposition. Add an assembly case with a real packed descriptor and the runtime. Device proof should cover a settings write through the project overlay and one queued operation on DT2 1.17 and DN2 1.12. No custom LFO protocol is needed to test the design on paper: a module-local editor can upload a shape asset or invoke a bounded validation/apply operation, with shape semantics supplied by the LFO service.
