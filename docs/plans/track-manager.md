# Track manager

Status: active. Branch `feat/track-manager` (lane `ext-model`), sub-branches in lane `lfo-shapes` merged into it.

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
   `shortcuts` service owns a hook at the entry of the stock key dispatcher. Rebinding comes later.
4. **Name length 15 characters.**
5. **The runtime learns no feature.** It gains only one generic device fact (track count). Shortcuts, views and names stay
   in the service, `core/ui` and the module.

## Parts

| Part | Layer | Owns |
| --- | --- | --- |
| `core/ui` (adapter) | L1 | Module views (full screen or overlay), the canvas, SETTINGS rows of modules, text input over the stock naming screen |
| `core/services/shortcuts` (new) | L3 | Binding records, held-key state, the key-dispatch hook, calls to the module on press and release |
| Device data + runtime | L2 data | `tracks` in each device file and `dk_device_track_count()` read from the device object |
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
  then `invert_rect`, `frame_rect`, `text_width`. Page code can take `dk_canvas` later with no ABI change.
- Views come from a fixed pool in `core/ui` (one owner object: open, close, close all of an owner). `core/ui` closes and
  frees a module's views and drops its SETTINGS rows when the module unloads, so a module with views stays unloadable
  (menus are not: `DK_E_RELOAD_MENUS`).
- SETTINGS rows: one registry (`SettingsRows`, at most 4 rows). The existing hook `cf.menu.hook.settings_list.tail` adds
  the app row (`dk_app_settings_rows`) and then the registered rows, in load order.

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

- Keys are logical (`dk_key`, `DK_KEY_TRK`, `DK_KEY_YES`, `DK_KEY_TRIG(n)`). Their names come from the device file
  (`[panel]` gets a name for each code); one generator writes the C names and the per-device map. No module holds a panel
  code.
- Owner objects of the crate: `KeyState` (held keys, a bit set, press and release), `BindingTable` (records by owner, the
  first owner keeps a binding, a later one is disabled and logged, `remove(owner)` on unload), `Shortcuts` (`on_key(event)`
  returns pass or swallow, keeps the one active binding, calls `on` and `off`). The hook only calls `Shortcuts::on_key`.
- While a binding is active, its keys and their releases are swallowed; all other events (trig keys under [TRK]) pass, so a
  track change while [TRK] is held works as stock.

### Device fact

`tracks = 16` in the device files (both targets; Syntakt 12 later). `dk_device_track_count()` returns it. The module draws
rows of 8 from it; no literal 16 in the module.

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
| Unit | `KeyState`, `BindingTable` (first owner wins, remove on unload), `Shortcuts::on_key` (swallow, pass, release order, trig keys under [TRK]) | `core/services/shortcuts` crate tests over `FakeCore` |
| Unit | View pool (open, close, close all of an owner, pool full), `SettingsRows` (order, cap, unload) | `core/adapters/ui/host:rust-tests` (`dkui_host`, C over fakes) |
| Unit | `track_names` codec (round trip, empty, bad version, 15 characters), `grid_cursor` | module `test-host` (cc-built C, as `modules/tools/remote/tests/pacing.test.ts`) |
| Contract | `shortcuts` runs the service contract suite (load, unload, reload leave the same state) | `core/services/shortcuts/tests/contract.rs` |
| Assembly | the `shortcuts` service file and its hook declarations; a module with a `shortcut` record; a card without the service refuses that module | `core/runtime:test-assembly` |
| Device | [TRK]+[YES] calls `on`, YES does not reach the stock view, releasing calls `off`; a trig under [TRK] still selects | `core/services/shortcuts` device test (fixture module) |
| Device | SETTINGS row opens a module view; text input round trip; view closed on unload | `ui-demo` fixture device test (extended) |
| Device | name a track, reboot, the hint shows the name | `digikit/track-manager` device test |

Device tests run on DT2 and DN2, with no DSP.

## Phases

| # | Work | Lane | Status |
| --- | --- | --- | --- |
| 0 | Static: anchors for the view base, the naming screen (both devices, control block, NO behaviour) and the key-dispatch hook site; second-agent byte check; one finding. Emulator look: hold [TRK] on DT2 and DN2. | static | active |
| 1 | `core/ui`: view pool, canvas, SETTINGS rows, logical keys, unload; host tests; `ui-demo` extended | `lfo-shapes` (sub-branch `feat/track-manager-ui`) | |
| 2 | `core/ui`: `dk_ui_text_input` | same | |
| 3 | `shortcuts` service: crate, record kind, dispatch hook, contract and assembly cases; device fact `tracks` | `ext-model` | |
| 4 | `digikit/track-manager`: module, unit tests, device test | `ext-model` | |
| 5 | Review, device gate (new device tests, gate-smoke, gate-pins on DT2 and DN2), PR | | |

Phases 1-2 and 3 run in parallel; shared files (`sdk/sdk.toml`, generated headers, assembly tests, workspace
`Cargo.toml`) are merged by regenerating, not by hand. Each phase gets an independent review before its device run.

## Not in scope

Rebinding, the shortcut list and help page, clash reports to the user, a names service, the per-project switch before
`svc/project` merges, Syntakt.

## Follow-ups

- A shortcuts page (DIGIKIT > SHORTCUTS or a module): list from the service's records, rebind, show clashes.
- `dk_page_canvas_v1` users move to `dk_canvas`.
