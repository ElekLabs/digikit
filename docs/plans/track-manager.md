# Track manager

Status: phases 0 to 4 are done and host-green on branch `feat/track-manager`; phase 5 (the device proof) is pending. Findings: [155](../findings/155-key-event-path-module-views-and-naming-screen.md).

## Goal

The user names tracks (track 1 KICK, track 9 BASS, ...) in a new SETTINGS entry, and a held key combination shows the name of
the active track while it is held. Three reusable pieces come with it: module-owned screens and text input in `core/ui`, a
`shortcuts` service that any module can bind key combinations through, and one device fact (track count) for modules.

## Decisions (Em, 2026-11-20)

1. **Names live in the module's own settings** (`dk_settings`, key `digikit/track-manager/names`). No names service. With the
   project service (branch `svc/project`) module-owned keys become per-project with no module change; until it merges, names
   are device-wide. A names service comes only when a second module needs the names.
2. **A new L3 service `shortcuts`.** It owns the key bindings of all modules. A module declares a binding in `init` with
   `dk_shortcut_add` (a record of kind `shortcut`), not in `module.toml`: the record path needs no runtime change, and the
   service holds every binding in one place, so a later list, help page or clash report reads them from there.
3. **Default binding [TRK]+[YES].** The stock `TrackSelectionView` (pushed while [TRK] is held) consumes only TRIG 1-16,
   REC, PLAY, STOP and, in one case, NO; every other key goes on to the view below (finding 122, static read of
   `vfunc_2`, DT2 `0x40078020`, DN2 `0x40072510`). So a combination is only clean if its last key is swallowed: the
   `shortcuts` service owns a hook at the entry of the stock view walker (finding 155). The hook only decides; the module's
   `on` and `off` run in the service's tick. Rebinding comes later.
4. **Name length 15 characters.**
5. **The runtime learns no feature.** It gains only one generic device fact (track count). Shortcuts, views and names stay
   in the service, `core/ui` and the module.

## Parts

| Part | Layer | Owns |
| --- | --- | --- |
| `core/ui` (adapter) | L1 | Module views (full screen or overlay), the canvas, SETTINGS rows of modules, text input over the stock naming screen; owner handles; closes a module's views and drops its rows when the runtime's unload observer says the module goes; defers a push until the release of the key that asked for it |
| `core/services/shortcuts` (new) | L3 | Binding records, held-key state, the key hook, the queue of `on` and `off` calls that its tick makes |
| Device data + runtime | L2 data | `tracks` and `[input.keys]` in the device files; `dk_device_track_count()`, `dk_selected_track()` and the key map read from the device object |
| `digikit/track-manager` (new tool module) | L4 | The names, the manager screen, the naming flow, the hint overlay |

## API (SDK additions)

Names show the owner; verbs say what happens.

### `core/ui`

```c
// init only: a row at the end of SETTINGS that calls open(arg); removed when the module unloads
int dk_ui_settings_row(dk_ui *ui, const char *text, void (*open)(void *arg), void *arg);

// a screen the module draws; DK_UI_VIEW_OVERLAY: drawn over the stock screen, every key goes on to the stock views
typedef struct {
    uint32_t flags;                                          // DK_UI_VIEW_OVERLAY
    void (*draw)(void *arg, const dk_canvas *c);             // every frame
    int  (*key)(void *arg, dk_key key, uint32_t flags);      // full screen only; 1 = consumed
    void (*closed)(void *arg);                               // after the view left the stack
    void *arg;
} dk_ui_view_desc;
dk_ui_view *dk_ui_view_open(dk_ui *ui, const dk_ui_view_desc *d);
void dk_ui_view_close(dk_ui_view *v);

// the stock naming screen; done(arg, text) on YES only, nothing on NO
typedef struct {
    const char *title, *initial;
    uint32_t max_len;                                        // at most 15 here; the screen's own limit is checked
    void (*done)(void *arg, const char *text);
    void *arg;
} dk_ui_text_input_desc;
int dk_ui_text_input(dk_ui *ui, const dk_ui_text_input_desc *d);
```

- `dk_canvas` (new header `dk/platform/canvas.h`): `width`, `height`, `text`, `fill_rect` in the order of `dk_page_canvas_v1`,
  then `invert_rect`, `frame_rect`, `text_width`, and the font choice `text_in(x, y, font, str)` and `text_width_in(font, str)` (`DK_FONT_SMALL`, `DK_FONT_LARGE`: the large ROM font, box height 15, anchor `cf.text.data.font_large`). Page code can take `dk_canvas` later with no ABI change.
- Views come from a fixed pool in `core/ui` (one owner object: open, close, close all of an owner). `core/ui` closes and
  frees a module's views and drops its SETTINGS rows when the module unloads, so a module with views stays unloadable
  (menus are not: `DK_E_RELOAD_MENUS`).
- SETTINGS rows: one registry (`SettingsRows`, at most 4 rows). The existing hook `cf.menu.hook.settings_list.tail` adds
  the system package's rows (`dkrt_ui_settings_rows`: the `ui.settings_rows` hook, the DIGIKIT row) and then the registered rows, in load order.

### `shortcuts`

```c
typedef struct {
    const char *id;            // unique within the module, e.g. "hint"
    const char *label;         // for a later list and help page, e.g. "TRACK NAME HINT"
    dk_key keys[2];            // default binding: hold keys[0], press keys[1]
    void (*on)(void *arg);     // the combination is complete (keys[1] was swallowed)
    void (*off)(void *arg);    // either key released
    void *arg;
} dk_shortcut_def;
int dk_shortcut_add(dk_module_ctx *ctx, const dk_shortcut_def *d);   // init only
```

- Keys are logical (`dk_key`, `DK_KEY_TRK`, `DK_KEY_YES`, `DK_KEY_TRIG(n)`): `[keys] order` in `sdk/sdk.toml` is the SDK's
  append-only list of names (the order is the ABI). A device file maps a name to its panel code (`[input.keys]`) and may use
  only listed names; `patchkit gen` writes `sdk/include/dk/gen/keys.h`, the Rust constants, `DeviceProfile.key_codes` and
  `dkrt_dev` `key_codes` from them. A key a device lacks maps to 0. SRC is one key on both devices (the DN2 panel prints SYN).
  No module holds a panel code.
- Owner objects of the crate: `KeyState` (held keys and the keys whose press was swallowed), `BindingTable` (records by owner,
  the first owner keeps a binding, a later one is disabled and logged, a binding with a key the device lacks is inactive,
  `remove(owner)` on unload), `Shortcuts` (`on_key(code, flags)` returns pass or swallow, keeps the one active binding and
  queues `on` and `off`; `run_pending` makes the calls). The hook calls `on_key` through `dk_shortcuts_key`; `Service::tick`
  calls `run_pending` in the UI pass. A queued call of a module that unloads is dropped.
- While a binding is active, its second key, its repeats and its release are swallowed; the first key and all other events
  (trig keys under [TRK]) pass, so a track change while [TRK] is held works as stock.

### Device fact

`[digikit] tracks = 16` in the device files (both targets; Syntakt 12 later). `dk_device_track_count()` returns it and
`dk_selected_track()` returns the selected track (0 to count - 1, or -2 without a project; the same firmware read as
`dk_debug_selected_params`). The module draws rows of 8 from the count; no literal 16 in the module.

## Screens (128 x 64)

SETTINGS gets a row `TRACK MANAGER` at the end of the list.

```
+--------------------------------+
|TRACK MANAGER                   |
| 01 02 03 04 05 06 07 08        |
| 09 ## 11 12 13 14 15 16        |   ## = selected (inverted)
| --       -- --                 |   bar under a number = named
|                                |
|10  BASS                        |
|[YES] RENAME          [NO] EXIT |
+--------------------------------+
```

- [LEFT] [RIGHT] one track, wrapping; [UP] [DOWN] the other row; [TRIG n] jumps to track n; [YES] opens the naming
  screen; [NO] closes. The screen opens on the active track.
- Naming screen: title `TRACK 10 NAME`, the current name, at most 15 characters. YES saves; NO leaves the name. An empty
  name removes it.

Hint, while [TRK]+[YES] is held (over the stock screen; the stock track selection keeps working):

```
+--------------------------------+
|                                |
|   (stock screen)               |
|  +--------------------------+  |
|  | 10  BASS                 |  |
|  +--------------------------+  |
+--------------------------------+
```

A track with no name shows `TRACK 10`.

## Module

`modules/tools/track-manager`, id `digikit/track-manager`, `category = "tools"`. Owner objects:

- `track_names`: `load`, `name_of(track)`, `rename(track, text)`, `save`. Value: version byte 1, track count, then per track a
  length byte and up to 15 characters. A bad or older value loads as no names.
- `grid_cursor`: `move(dir)`, `jump(track)`, `track()`; rows of 8 from the track count.
- `manager_screen` (full-screen view) and `hint_overlay` (overlay view, opened by the binding's `on`, closed by `off`).

## Tests

| Layer | What | Where |
| --- | --- | --- |
| Unit | `KeyState`, `BindingTable`, `Shortcuts` (swallow, pass, release order, deferred `on`/`off`, unload drops pending, inactive keys) | `core/services/shortcuts/tests/state.rs`, `records.rs` over `FakeCore` |
| Unit | the key hook's flag translation | `core/services/shortcuts/dks/src/hooks.rs` |
| Unit | generator: key numbers, key map, profile and `dkrt_dev` agree; refusals | `packages/patchkit/tests/devdata.rs` |
| Unit | View pool, `SettingsRows`, owner handles | `core/adapters/ui/host:rust-tests` (`dkui_host`, C over fakes) |
| Unit | `track_names` codec, `grid_cursor` | `modules/tools/track-manager/tests/logic.test.ts` (cc-built C, run by the inferred `test-host` target) |
| Contract | `shortcuts` runs the service contract suite | `core/services/shortcuts/tests/contract.rs` |
| Assembly | the service file's hook and kind, the fixture's needs, a card without the service refuses it | `core/runtime:test-assembly` |
| Device | [TRK]+[YES] calls `on` and `off`, SETTINGS does not get YES (view stack), YES alone passes, [TRK]+[TRIG 3] selects | `core/services/shortcuts/tests/device/shortcuts.device.test.ts` (fixture `test/shortcut-demo`) |
| Device | SETTINGS row opens a module view; text input; closed on unload | `tests/fixtures/modules/ui-views/tests/live/ui-views.device.test.ts` |
| Device | name a track, restart, the name is on the card, [TRK]+[YES] shows the overlay | `modules/tools/track-manager/tests/track-manager.device.test.ts` |

Device tests run on DT2 and DN2, with no DSP.

## Phases

| # | Work | Status |
| --- | --- | --- |
| 0 | Static: anchors for the view base, the naming screen and the key hook site; second-agent byte check; finding 155 | done |
| 1 | `core/ui`: view pool, canvas, SETTINGS rows, logical keys, unload; host tests | done, host-green |
| 2 | `core/ui`: `dk_ui_text_input` | done, host-green |
| 3 | `shortcuts` service: crate, record kind, key hook, contract and assembly cases; device facts `tracks`, `[input.keys]`, `dk_selected_track` | done, host-green |
| 4 | `digikit/track-manager`: module, unit tests, device test | done, host-green |
| 5 | Review, device gate (the three device tests above, gate-smoke, gate-pins on DT2 and DN2), PR | pending |

Phases 1-2 and 3 ran in parallel; shared files (`sdk/sdk.toml`, generated headers, assembly tests, workspace
`Cargo.toml`) are merged by regenerating, not by hand. Each phase gets an independent review before its device run.

## Not in scope

Rebinding, the shortcut list and help page, clash reports to the user, a names service, the per-project switch before
`svc/project` merges, Syntakt.

## Follow-ups

- A shortcuts page (DIGIKIT > SHORTCUTS or a module): list from the service's records, rebind, show clashes.
- `dk_page_canvas_v1` users move to `dk_canvas`.
- Rebuild the SETTINGS list on module load and unload (call the stock rebuild; needs a static check of the rebuild entry): today a stock row finds its row by the module's name and row number, so rows survive a reload, a stale row of an unloaded module does nothing, and the row of a module loaded live for the first time appears at the firmware's next rebuild.
