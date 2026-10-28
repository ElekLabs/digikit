# SDK API and module build

Status: agreed with Em on 2026-10-27.
This plan replaces step 9 (UI runtime services) of
[firmware-decls-and-sdk.md](firmware-decls-and-sdk.md) and the prebuilt-object part of its step 8.
It comes before the own-DSP work (`sdk/dsp`, the C-on-SHARC replacement).

Tick each item with its commit when it lands. Stop at the end of each phase for Em's review;
no commit without Em's OK.

Each migration deletes what it replaces in the same phase: the old manifest sections, their
generated files, the code that read them and the docs that describe them. Fixtures move too.
A manifest that still uses a removed form is refused with a message that names the new form.

## Goal

- Module authors write C against a stable, device-agnostic API of macros and functions
  for machines, effects, parameters, pages, menus and dialogs.
- The API calls a per-device adapter in `core/*`. The adapter uses the generated firmware
  bindings (`devices/fw/` to `modules/core/fw/`) and the device profile. A new device adds an
  adapter and anchors; it changes no module and no SDK header.
- The runtime menu (C) uses the same API as content modules. The runtime core stays in Rust.
- `module.toml` holds metadata only.
- A `.dkmod` is a source package. The desktop and web Transfer pages compile it, pack a `.dkm`
  and send it, with the same code (Rust, and the same code built for wasm).

## Decisions

1. Static module structure (machines, parameters, pages, FX, knobs, a module settings menu) is
   declared with C macros. The macros put versioned records in a `.dk.desc` section. The packer
   lowers them to the `.dkm` register records used today, so the runtime does not change.
   Every machine and effect has a small C file for this.
2. Run-time UI (confirm, choice, toast, open a menu, `dk_ui_has(cap)`) is reached through named
   runtime imports. A missing feature resolves to 0 and `dk_ui_has` returns false.
3. One spec, `sdk/sdk.toml`, generates the C headers, the runtime import table and the SHARC
   `.inc` files. It stays one file: about 50 tools and crates read it.
4. `core/ui` is the UI adapter, in C, one per device, on the generated `dk_fw_*.h` bindings.
   `core/digikit-menu` is rewritten in C on `<dk/ui.h>` and is its first user. The runtime
   (loader, store, upload, arena) stays in Rust. Reason: fast gcc builds, the adapter is
   firmware glue where Rust would be mostly `unsafe`, and one API for core and content modules.
5. The Rust runtime and the C UI layer share one ABI spec, `abi.toml` (new) in `modules/core/runtime/`.
   `patchkit gen` writes a C header and a Rust `gen_abi.rs` from it, with size and offset
   asserts on both sides and a compile-time signature check of each Rust export.
6. `module.toml` keeps id, version, name, author, summary, category, status, ports, requires and
   conflicts. Sources are found by convention: `src/*.c` is ColdFire, `src/dsp/*.s` is SHARC.
   `[[object]]`, `[[op]]` and `[[claim]]` remain for `core/*` only.
7. A `.dkmod` holds source only, never prebuilt objects. The receiver compiles it, so what runs is
   what the user can read.
8. Objects stay `.dko.json` (decided 2026-10-28: a binary `.dko` gains little once content objects
   are not committed; the device only sees the `.dkm`). Content-module objects are build output in
   `out/` and are not committed. `core/*` objects stay committed.
9. ColdFire C stays on gcc 16.2 `m68k-elf`. For the browser, first try gcc `cc1` and `as` built
   for wasm; fall back to an own C compiler for ColdFire in Rust.
10. Content modules follow module-sdk.md D1 (knob slots by position) and D4 (one compiled module
   for every device). The adapter decides per device where pages go.

## Layers

```
C module  --  <dk/fx.h> <dk/machine.h> <dk/ui.h>
  |  DK_* macros write .dk.desc records; dk_ui_*() calls are named imports
  v
.dkm  --  register records (lowered from .dk.desc) + imports
  v
core/runtime (Rust)  --  import table -> core/ui
  v
core/ui adapter per device (Rust, on dkfw and the device profile)
  v
firmware: VerticalMenuView, MenuItem, ConfirmWindow, MultiChoiceWindow, Toast, parameter pages
```

## Phases

### Phase 1: spec, headers and descriptors; freqshift

- [x] Generated headers move to `sdk/include/dk/gen/` behind hand-written `sdk/include/dk/*.h`;
      SHARC includes to `sdk/asm/`, vectors to `sdk/vectors/`; `sdk/README.md`.
- [x] Define the `.dk.desc` record format in the spec (`[desc]` of `sdk/sdk.toml`, generated
      `<dk/gen/desc.h>`), with `_Static_assert` on every layout.
- [x] `DK_FX`, `DK_KNOB` and `DK_KNOB_AT` macros in `<dk/fx.h>`.
- [x] The packer reads `.dk.desc` records (`packages/patchkit/src/desc.rs`) and lowers them to
      register records; `patchkit desc MODULE` prints the effects as JSON.
- [x] freqshift: `src/module.c` replaces `[[fx]]` and `[[fx.knob]]` in `module.toml`; the module's
      `src/` is an include directory for C and SHARC+ (shared `src/freqshift.h`). fx template too.
- [x] Proof: the freqshift `.dkm` is byte-identical to the manifest build on dt2 and dn2; every
      rebuilt object differs only in `source_sha256`.
- [x] Cleanup: the five `dyn-fx-gain*` fixtures use `DK_FX`; the manifest `[[fx]]` form is deleted
      (a manifest that has it is refused), with modgen's fx outputs and `pk_conform`'s manifest
      branch. All ten fixture `.dkm` files are byte-identical.
- [ ] Follow-up: an editor config (clangd) with the SDK include paths, so authors see no false
      errors in `module.c`.

### Phase 2: machines, params and pages in C

- [x] `DK_MACHINE`, `DK_PARAM` (knob units 0..127) in `<dk/machine.h>`; `DK_PAGE`, `DK_KNOB`,
      `DK_KNOB_AT` in `<dk/page.h>`, shared by machines and effects; `DK_FX_PARAM` in `<dk/fx.h>`.
- [x] Placement per device comes from the device profile: word w sits at `param_slots[w]`
      (Digitakt II 1, 2, 4, 5; Digitone II 0..3). `clone_of` is `[modules.knobs] clone_of` of the
      profile. `ports` is the only device restriction (`pack` and conformance check it).
- [x] pluck, bd, analog, pd, freqshift, the templates and the fixtures use C records. Removed:
      `[[voice]]`, `[[machine]]`, `[[param]]`, `[[page]]` (refused), clone_of, position, own_dsp,
      level, stock knob codes, per-device tables, templates, data-only machines, the generated
      `consts.rs`, `<dev>.inc` and `module.h`, and the fixtures test/xslice, twin-xslice, dyn-xslice.
- [x] Proof: `.dkm` files of pluck, sine, dyn-dsp-hang, dyn-guard-nan (dn2) and every effect are
      byte-identical on dt2 and dn2; patched images of bd, analog, pd and patched-pluck (dt2) are
      byte-identical. Changed on purpose: patched-pluck on dn2 (no position), dyn-dn2 and twin-dn2
      (sine DSP), dyn-params (4 params), dyn-guard-nan on dt2 (param on both devices).
- [x] Emulator re-checks (1eb92a86 + fixes): every RECHECK value confirmed but equiv-dn2-set
      `voices = 2`; scenarios equiv-dn2-set, memaudit-dn2, equiv-mixed, roundtrip-dt2 (3) and
      roundtrip-dn2 pass; the smoke cells of patched-pluck and machine-clone pass. Found and fixed: a
      patched-in machine on the Digitone II needs a list position (`[modules.knobs] list_position`
      of the device file), and the `pk_break rows` teeth test rebuilds its copied module.
- [ ] Later: per-machine knob maps in core/dsp-voice (free encoder placement, more than 4 params,
      several pages per machine); several pages per effect in core/fxpages; a module type that
      patches stock machines.

### Phase 3: `<dk/ui.h>`, `core/ui` in C, and the runtime menu in C

- [x] `abi.toml` (new) in `modules/core/runtime/`: the `dkrt_*` exports and structs. `patchkit gen` writes
      the C header and the Rust `gen_abi.rs` (layout asserts, signature checks). Card scan,
      compatibility, needs and fit move from the menu to runtime exports (`dkrt_scan`, `dkrt_fit`).
- [x] Anchors in `devices/fw/` for ConfirmWindow, MultiChoiceWindow, Toast and text drawing,
      checked on DT2 1.17 and DN2 1.12.
- [x] Trace the toggle menu item, value formatting and the key ids for YES and NO: 10 is YES, 12 is NO
      (finding 71). `fw.py dossier` and a faster `emit-rule` came out of this work.
- [x] List-view anchors (trace of the stock PRESET MANAGER): `MenuWithPopoutSideBarsView` (title bar,
      side panel on RIGHT), `CompactVerticalMenuView`, the check marks of `Menu` (check, toggle, select
      all, count), `setTitle`, `setTitleBar`, on DT2 1.17 and DN2 1.12.
- [x] `<dk/ui.h>`: menus, text pages, lists (title bar, check column, side panel, empty text), rows
      (action, submenu, text, toggle, value), confirm, choice, toast, `dk_ui_has(cap)`.
- [x] `core/ui` C adapter on `dk_fw_*.h`. Menus are built once and kept; dialogs are freed by the
      firmware after they close; `std::function` captures are two words, no heap.
- [x] Host build of `<dk/ui.h>` with a fake adapter that records calls and drives callbacks, for unit tests.
- [x] `core/digikit-menu` rewritten in C on `<dk/ui.h>`, in the new design (no pixel-identical
      intermediate step):
  - SETTINGS > DIGIKIT stays a vertical menu: ABOUT, HEALTH, MEMORY, MODULES. Glyphs later.
  - MODULES is a list like PRESET MANAGER: a title bar; rows of name, kind and size; a check mark
    for "load at next restart" (YES toggles it); RIGHT opens a side panel with LOAD / UNLOAD,
    ROUTE... (effects only: a choice of SEND or MASTER) and INFO (size, DSP %, CPU %, fit).
    The empty text is "NO MODULES ON CARD".
  - NO with changes pending asks "APPLY & RESTART?": YES applies, NO discards.
  - A module that does not fit gives a toast "NO FIT: NAME".
  - The FX ROUTES screen is removed; the fx-routes scenario uses ROUTE....
- [x] Proof: host tests; no firmware-heap growth after the menu, a list and a dialog open and close
      50 times; new golden frames approved by Em from screenshots on dt2 and dn2.

### Phase 4: UI for content modules; remote

Layers (docs/ARCHITECTURE.md): a module writes to the SDK; the runtime links the call when the module
loads and owns policy; the adapter (core/ui) carries it out. The runtime owns enable and disable (the
MODULES check marks); a module owns only its own settings.

- [x] `dk_ui_*` are `[[import]]` entries of `sdk/sdk.toml` provided by core/ui. When core/ui is absent the
      runtime hands out stubs that return the API's failure value (0, -1 or null) and `dk_ui_has` is
      false; the module still loads.
- [x] A module settings menu: the module exports `void dkm_menu(dk_ui_menu *m, void *ctx)` (`DK_MENU(fill)`
      is sugar). `DkrtModule` gets a `menu` field. The MODULES side panel shows SETTINGS... for a loaded
      module with a menu. The DIGIKIT menu calls `dkm_menu` only when the user opens it.
- [x] REMOTE: the check mark in MODULES is the only switch. Delete the REMOTE switch, the `E_DISABLED`
      guard of the remote range, the REMOTE row and `core/remote-open` (and its layercheck entry); the
      emulator's test card selects remote. `digikit/remote` gets its own settings (what it owns, e.g.
      held keys, RELEASE ALL KEYS).
- [x] Proof: host tests (remote's menu, the stubs); dk:dev screenshots on dt2 and dn2; a patch set
      without core/ui still loads remote; the remote scenarios and the menu proof pass.

### Phase 5: `patchkit build` in Rust

ColdFire builds move to Rust. SHARC stays in `pk_sharc.py` until 6a (its fix-ups need the Python
SHARC decoder; the own assembler of 6a replaces them). gcc, as, ld, objcopy, cargo and selas stay
child processes.

- [x] 5a. `patchkit build MODULE`: C, asm and Rust crates for ColdFire in Rust (ELF through the
      `object` crate, the ISA check in-process through the `coldfire` crate, the same `source_sha256`);
      SHARC through `pk_sharc`. `patchkit dko show FILE`. Proof: every module's and fixture's
      `.dko.json` is text-identical to the Python build. Then `pk_build`, `pk_link`, `pk_rust` and the
      ColdFire part of `pk_elf` are deleted and their callers use `patchkit build`.
- [x] 5b. `patchkit build` and `pack` share the object in memory: a content module goes from source to
      `.dkm` in one command. Content-module and fixture `.dko.json` become a cache in `out/` (by source
      hash; used by `module:pack-all`, the Rust tests and the web gallery); only `core/*` objects stay
      committed (the workbench and the browser patch without a toolchain).
      Also: `serde_json`'s pretty printer replaces `canon.rs` (one reformat of committed objects if
      needed); `isacheck.py` calls patchkit instead of its own ISA allow-list.
      Proof: every packed `.dkm` and patched image is byte-identical.
- [x] 5c. The workbench Reload modules button builds first (ColdFire in-process; a SHARC change says
      to run `dk:dev`).

### Phase 6: build in the browser

- [x] 6a. SHARC in the browser: first build Selache's `selas` (Rust) for wasm and port the wrapper
      (`pk_sharc`: encoding-bug guards, width fix-ups, cross-check) to Rust. Only if that fails, an own
      SHARC+ assembler in Rust. Proof: byte-identical against the native build on every module.
- [x] 6b. Spike, at most one day: gcc `cc1` and `as` for `wasm32-wasi`. If it fails, plan an own
      C compiler for ColdFire in Rust. Result: gcc `cc1` and `as` build for wasm32-wasip1 and give
      byte-identical output on 32 module sources (finding 74); ship them. `ld` for wasm is next (6c).
- [ ] 6c. Transfer in the site (decided 2026-10-28): the workbench's Transfer pane moves to shared code
      (`apps/web/src/transfer/`, a `TransferEnv` for library, targets and desktop-only extras) and
      becomes the web Transfer page. The site ships each module's prebuilt `.dkm` per device (built at
      site-build time, the same bytes as `module:pack-all`), an index and the sources (read-only source
      view). The Upload page folds into Transfer ("Add .dkm files"); the Patch page patches the runtime
      set only. A connection indicator (connected, connecting, not connected; device, OS, runtime, id)
      in the shared pane. WebMIDI: cancel, disconnect detection, clear messages without WebMIDI or
      SysEx permission. Proof: desktop Transfer unchanged; web tests; Em's test in Chrome on the device.
      Status: built (shared pane, site assets, web Transfer page, WebMIDI hardening, badge with OS
      version); Em's device test waits for 6d (the device needs a flashed runtime first).
- [ ] 6d. Patch and flash, one path for the CLI, the workbench and the web (finding 75: the OS UPGRADE
      screen takes the plain `.syx` stream, sends no replies, validates checksum, HMAC and on DT2 the
      build floor "0060" after the last message, erases nothing on failure, asks no YES).
      - [x] Core `patchkit::flash`: `prepare` (stock by SHA-256, patch the `device` set, preflight,
            verify, DT2 build floor, product byte, the outgoing message list decoded back and compared
            with the image); restore stock through the same checks; a `Sender` with 2 ms pacing that
            stops on any error (start again from the header). CLI `patchkit flash STOCK.syx --port`
            and `--restore`. USB only for now.
      - [x] Image id: the runtime's INFO reports an id of the flashed image (set by patchkit), checked
            after the reboot.
      - [x] One step UI (Firmware page) for the desktop (midir) and the web (`patchkit.wasm` + WebMIDI);
            the web Patch page becomes Firmware. Recovery steps shown first; device match before
            flashing. No emulator boot before flashing (decided 2026-10-28).
      - [x] Proof: unit tests of every check (tampered, wrong device, oversize); a full flash through
            the shared sender into the emulated DT2 and DN2 (tools/emulator/os_upgrade_rig.py), the
            flash compared with the image, the emulator boots the patched runtime and INFO's id
            matches; screenshots. The Python dkflash design (device-bridge.md section 3) is replaced.
      - [x] RPC route (finding 75, "The RPC route"): one reply-driven state machine in Rust
            (`patchkit::flash_rpc::RpcFlash`: Start, Write in order, End; CRC retry, timeouts, End 0 on
            failure or cancel) for the CLI, the workbench and the site (wasm). Default route; the menu
            route stays as the fallback (`--route menu`, a link on the page).
      - [x] The device does not ask "UPGRADE OS NOW?" on this route: the CLI asks once before Start
            ("Flash Digitakt II OS 1.17 ... now? [y/N]") and the Firmware page has one Flash button; both
            commit (End 1) as soon as the device has checked the image. Cancel or leaving the page before
            that sends End with commit 0.
      - [x] The Firmware page without the menu step and its box: one Flash button for send, check and flash, then Done; errors in plain
            words (CRC retries used up, timeout, the device's message, "Nothing was written").
      - [x] Proof in the emulator through the page's own code path: DT2 1.17 and DN2 1.12, patch and
            restore (flash equals the container, INFO runtime and image id), Cancel before the commit leaves
            the flash unchanged; the CLI once with `--yes`. Open: the real device (largest Write,
            timing).
- [ ] Later: compile in the browser (a dropped `.dkmod`, or an edited source): gcc `cc1`/`as`/`ld` in
      wasm (finding 74) in a worker, SHARC+ already in `patchkit.wasm`.
- [ ] Optional: a `.dkmod` carries the expected `.dkm` hash per device so a receiver can
      compare builds.

### Phase 7: migrate the rest

- [ ] bd, analog, pd, master-gain, master-drive.
- [ ] Remove the typed `module.toml` sections and the legacy Python build tools.
- [ ] Proof: byte-identical patched images.

### Dev loop

- [x] `dk:dev` and the daemon method `dev.reload`: UI-layer modules are written into a running emulator
      (about 4 s per edit); any other change boots (DN2 5 s, DT2 10 s). Reload modules button in the
      workbench. `tools/emulator/boot_phases.py` measures the boot.
- [ ] `mise run dk:watch`: rebuild on save and call `dev.reload` on the open workbench's control socket.
- [x] After phase 5: the workbench Reload modules button builds first (`patchkit build` in Rust).
- [ ] Cache the module library scan in `dev.reload` (1.2 s of each reload).

### Later

Own DSP work (`sdk/dsp`, the C-on-SHARC replacement), text entry, module-owned views and the
Syntakt adapter (S3, S4).

## Status at end of session

2026-10-27: plan agreed. Phase 1 done in ae364ec3. Phase 2 done in 1eb92a86 and its follow-up. Phase 3 started: the ABI spec, the runtime exports and the UI firmware anchors are done; the
menu is unchanged (goldens match). Then the list-view anchors, `<dk/ui.h>`, core/ui (demo fixture
`test/ui-demo`, screenshots and a flat heap on dt2 and dn2) and the host harness `dkui-host`.
Then the digikit menu in C on core/ui (the Rust menu crate is deleted; core/ui is in the
runtime sets of both devices); Em approved the new goldens. Phase 3 done. Then core modules grouped by role
(`adapters/`, `apps/`, `role` in module.toml), docs/ARCHITECTURE.md and `mise run check:layers`. Phase 4 done (module UI through runtime imports,
`DK_MENU`, REMOTE switch removed). Phase 5 done (`patchkit build` in Rust, content objects only cached, Reload
modules builds first). 6a done (SHARC built in Rust with Selache as a
library, natively and in wasm); 6b done (gcc in wasm, finding 74). Next: 6c.
