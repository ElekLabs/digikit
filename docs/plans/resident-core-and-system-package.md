# Resident core and system package

Date: 2026-11-22. Status: **approved by Em 2026-11-22.** Item **RC2s** of [runtime-contract-and-lifecycle.md](runtime-contract-and-lifecycle.md).

Facts: static, from `patchkit size` groups of the RC2b-2 runtime object (text and data 274,498 bytes), grep and the findings named below. Mixed groups are estimates. Nothing was run on a device.

## 1. Goal

The flashed runtime keeps only what must work without the card or must repair the card. Everything else loads from the card as one **system package**. Then:

- The flashed image shrinks by about 167 KB. On the Syntakt each flashed byte removed saves one stored byte ([finding 159](../findings/159-syntakt-inplace-unpack-bound.md)), so the Syntakt fits with a wide margin.
- The package manager, the full loader and the DIGIKIT menu change without a reflash: an update is a Transfer upload and a restart.

## 2. The split

| Resident core (flashed, about 108 KB) | System package (card, about 167 KB) |
| --- | --- |
| Reset installer, platform hooks, the `Platform` implementation (firmware addresses stay in the image) | Package manager, package graph, resource ledger, the runtime's lifecycle seams |
| Boot counter, selection, safe-mode decision, crash and hang records (`fault`), the DSP hang watch (`watch.rs`, 2 KB) | Full loader: linking, records, imports (`loader`, `imports`, `entry`, `dkneeds`), DSP loading (`dsp`) |
| Store, store writer, storage, `hash32`, the Transfer commands that repair a card (SELECT, RESTART, UPLOAD, DELETE, READ, VERIFY, INFO) | The Transfer commands about loaded modules (MODULES, CAPACITY, LOADS, RELOAD, RELOAD_STATE, LOAD, UNLOAD) |
| Settings broker (the record is read at boot) | Planner and reload job, service registration (`svc_rows`), settings overlay, file listings for modules (`files`), scan and fit |
| `core/ui` (12.4 KB, the firmware views); the boot frame that shows a safe or core-only reason (`safe_screen.rs`, 678 B) | The DIGIKIT menu app |
| The DSP link transport (`dsp-link`, platform) | The DSP link queue (`dkrt-link`) |
| The **stage-1 loader**: about 2-3 KB on top of code that is resident anyway (`image` head, sections, copy and relocate, `arena`, store find, `sha256`) | |

The library code both sides use (about 15 KB of `dkrt_sdk` helpers and constants) is compiled into both. That is one source in two binaries, not copied source.

## 3. Boot

1. Reset, platform hooks, main-OS task entry: as today.
2. The resident core reads the selection, the boot counter and the settings record, and decides the level (section 5): normal, safe or core-only.
3. Stage 1 finds the system package on the card (a fixed store path), streams it through the staging buffer while hashing it with `sha256` (the package is about 167 KB, larger than the 128 KB stage), copies it to its place in the arena, checks the hash against the package header, relocates it, and checks that the package's resident-table version is compatible (same major, minor not newer than the core's).
4. Stage 1 calls the package's entry with the **resident table** and the level. The package registers its **hooks**. At the normal level it brings up the selection through the package manager; in safe mode it brings up no modules, only its own menu.
5. A missing, corrupt or incompatible package is **not** a crash: the boot counter does not count it, and the device runs at the core-only level.

## 4. The two interfaces

**Resident table** (core to package): one versioned `#[repr(C)]` table of function pointers that the package calls. About 65 functions today: boot record writers (11), `dlog` (11), platform (9, plus the 45 `Platform` trait methods, which become table entries), store and store writer (8), arena (3), polls, menu state, `sha256`. Grouped by owner object (`platform`, `store`, `boot_record`, `log`, `arena`), each group its own `#[repr(C)]` struct, so a group can grow by appending. The table carries `major.minor`; a new function is a minor bump appended at the end; a removed or changed one is a major bump.

**Hooks** (package to core): one small struct the package fills at entry: `tick(phase)`, `transfer_command(cmd, req, reply)` for the module commands, `settings_rows(menu)` for the SETTINGS hook (today's optional symbol `dk_app_settings_rows`), `boot_report()`. A null hook means "not loaded": the core keeps stock behaviour, as a zero slot does today.

The boundary is enforced in the tree before the binaries split (RC2s-1): `check:layers` refuses a call from resident code into package code except through the hooks, and a call from package code into resident code except through the table.

## 5. Levels and recovery

| Level | When | What runs | Way out |
| --- | --- | --- | --- |
| **Normal** | Default | The system package and the selected modules | |
| **Safe** | The boot counter reaches its first threshold (repeated crashes), the safe key is held, or a DSP hang was recorded | The system package and the DIGIKIT menu, no modules | The full menu as today: untick the module, APPLY & RESTART; or Transfer |
| **Core-only** | The system package is missing, corrupt or of an incompatible version (not counted as a crash), or the boot counter reaches its second threshold (crashes continue in safe mode, so the package or the core is the likely cause) | The resident core: stock firmware, Transfer, no DIGIKIT UI | The boot frame shows the reason for 4 s (`safe_screen.rs`); repair over Transfer (upload a matching system package, or reset the counter), then restart |

- The user's repair flow is unchanged: safe mode still has the menu. The resident core needs no interactive screen.
- The host repair path works at every level: Transfer is resident.
- The DSP hang watch stays resident (Em, 2026-11-22), so a DSP hang still leads to safe mode; with no modules loaded no module DSP code runs.
- The workbench offers "install the system package" in one action when a device reports core-only for a missing or mismatched package.

## 6. Memory

- The package takes about a third of the 512 KB arena. It sits at the top of the arena so module allocations below it are unchanged; the capacity report counts it as `core.package` (Em, 2026-11-22).
- The staging buffer stays 128 KB; stage 1 streams.
- The menu lifetime rule (menus are never freed) means the package is replaced only at the next boot, never at run time. That matches an update: upload, then restart.

## 7. Tests

- **Unit:** stage 1 over `FakeStorage` and `FakePlatform` (good package; bad hash; incompatible version; missing file: core-only, no boot count); the level decision (both thresholds, the safe key, a DSP hang); the resident table and hooks over fakes.
- **Assembly:** the real packed system package loaded by stage 1 on the host, its hooks registered, a module booted through it.
- **Device:** the smoke set, with the system package on the test card. The card preparation installs the built package for every device test, so no test names it. One new device test per device: core-only with the package removed, then recovery by a Transfer upload and a restart. The existing safe-mode device test checks that safe mode now shows the menu and loads no modules.

## 8. Order

| Step | What | Proof |
| --- | --- | --- |
| RC2s-1 | Draw the boundary in one binary: resident table and hooks as Rust types, package-side code reached only through them, `check:layers` rule. No behaviour change | host tests; layer check; runtime object code unchanged except the indirection |
| RC2s-2 | Build the system package as its own artifact; stage 1; the three levels and the second counter threshold; card preparation installs the package | unit and assembly tests; the smoke set and the new core-only device test on DT2 and DN2 |
| RC2s-3 | The DIGIKIT menu moves into the package (hook slot for SETTINGS) | menu device tests on DT2 and DN2; Syntakt image size checked |

RC2b finishes first (RC2b-2s stack fix, RC2b-3 one planner), so the code that moves is already one clean unit. RC2c (service reload and unload from the menu), RC2d (pins) and RC2e (MIDI snapshot) then land inside the package.

Estimate: RC2s-1 1-2 days, RC2s-2 2-3 days, RC2s-3 1 day. Confidence: medium; the recovery path (sections 3.5 and 5) is the part that must never fail.

## 9. Decisions

Taken by Em, 2026-11-22:

1. Safe mode loads the system package and its menu with no modules; core-only (resident core, no DIGIKIT UI) is the fallback when the package is missing, broken or keeps crashing. No resident interactive screen.
2. A missing or incompatible system package is core-only, not counted as a crash.
3. The DSP hang watch stays resident.
4. The package sits at the top of the arena, counted as `core.package`.
