# Module and service distribution

How modules, services and their Workbench pages reach users. Agreed 2026-11-28. [Module pages](workbench-module-ui.md), [Development guide](../DEVELOPMENT.md)

## Decisions

- The site installs built assets only: `.dkm` for content modules, `.dks` for services, the port files, and one prebuilt page bundle per module that has a Workbench page. The Pages deploy builds them (`.github/workflows/pages.yml` already installs the ColdFire toolchain). The browser compiles nothing.
- A module never carries a service. Its NEEDS table names each service by id and version; Transfer resolves the needs to the shared `.dks` files (`serviceRefs`, `packages/workbench-core/src/modules/dktransfer.ts`). One service file serves every module that needs it.
- `.dkmod` stays the redistributable source package and a Workbench feature: Workbench imports a `.dkmod` and builds it with the native toolchain. A service gets its own `.dkmod` (`type = "service"`), never inside a module's.
- A module page is built at publish time and never sent to the device. The site stores an installed page bundle in IndexedDB (`BlobStore`) keyed by module id, version and hash, and runs it in a sandboxed iframe that reaches the module through `ModuleChannel` over `postMessage`.
- Firmware patching still links objects in the browser, because the user's firmware image cannot be published patched. Those objects become a deploy-built asset (the `bundle.json` that `patchkit.wasm` reads) instead of committed files.
- Compiling C in the browser (gcc `cc1` and `as` as WebAssembly, [finding 74](../findings/74-gcc-wasm-spike.md)) is deferred until users bring their own source packages to the site.

## Work items

1. Generated-object merges (done with this plan): `.gitattributes` gives committed core objects the merge driver `generated-object`; `objects-check` forces the rebuild.
2. Stop committing core objects. First audit every reader of `{runtime,services}/**/build/*.dko.json` (patchkit tests, `objects-check`, device test keys and image hashes, Python tools, the desktop app from a checkout). Then build them in the Pages deploy into `bundle.json`, and locally through Nx and the shared cache. Payoff: no generated-file merges or rebuild commits in runtime PRs. Audit about half a day, change about 1 to 2 days.
3. Service source packages: `.dkmod` with `type = "service"`, export and import in patchkit (Rust, so Workbench and the CLI share it), import in Workbench. About 1 day, with or after item 2.
4. Page bundles on the site: publish one bundle per module page beside its `.dkm`, store on install, sandboxed iframe with the `ModuleChannel` bridge, the site's Modules sidebar. About 1 to 2 days, after the Project Manager page exists.

## Open

- **[O]** The site's Content-Security-Policy for sandboxed page iframes; the desktop app needs a custom scheme for them, like its symbols catalog.
- **[O]** Which runtime versions a published `.dkm` supports, and how the site picks among builds.
