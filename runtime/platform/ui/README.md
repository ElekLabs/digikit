# core/ui

The device adapter behind `<dk/platform/ui.h>` (`sdk/include/dk/platform/ui.h`): menus, text pages, lists with a title bar, check marks and a side panel, rows
(action, submenu, text, toggle, value), confirm and choice dialogs and toasts. C, one source for the Digitone II and the Digitakt II; every firmware
call is a generated anchor (`modules/core/fw/include/dk_fw_*.h`). The host fake of the same API, with its tests, is in `host/`; it also compiles the menu of `core/digikit-menu` against a fake runtime (`host/tests/menu.rs`).

- The `dk_ui_*` functions are plain global symbols (`dk_ui_open(ctx, &ui)` gives a module its handle; `dk_ui_menu_*` work on a menu; `dk_ui_menu_new`, `dk_ui_confirm`, `dk_ui_choice`, `dk_ui_toast` and `dk_ui_tick` take the handle first). A content module calls them as imports (`[[import]]` of `sdk/sdk.toml`, `requires = "core/ui"`): the runtime's loader links them by name, and hands out a stub that returns the failure value when this module is not in the image (docs/ARCHITECTURE.md). `host/tests/remote.rs` runs `digikit/remote`'s settings page on the host fake.
- A plain menu and a text page are the firmware's `VerticalMenuView`; a `DK_UI_LIST` is a `MenuWithPopoutSideBarsView` (the PRESET MANAGER's
  base) with the right panel as the side panel. Each is built once and kept for the session through a `shared_ptr` this module owns, so the
  firmware never frees it.
- `DK_UI_WIDE` (flag 4) builds a menu page as the list view without check marks or side panel: a full-width title bar (about 28 characters; the firmware cuts a longer title at its start with `..`) and full-width rows. `core/digikit-menu` sets it for module settings pages. A toggle or value row of a list or a wide page carries its value in its label (`TOGGLE            ON`, `VALUE              064`), because the list view does not give the item's draw function the row's origin; a change fills the list again.
- Rows are stock `MenuItem`s. Their `std::function`s keep two words (the menu and the row number) in the function's own storage: no heap
  per row. The row data lives in an array of the menu that a refill rewrites.
- The View vtable of a text page and of a list is a copy in module RAM: paint (slot 4) hides the cursor bar of a text page and draws the
  empty text of a list; key (slot 2) of a list adds RIGHT (side panel), YES (check mark) and the `back` callback, and sends the rest to the
  stock handler.
- Dialogs are `make_shared` blocks built by hand and pushed on the Brain's view manager; the firmware frees them. One at a time.
- `dk_ui_stock_row(stock_menu, text, fn, arg)` and `dk_ui_core_handle()` (the `dk_ui` handle, for a core module that has no `dk_module_ctx`) are for other core modules (`include/dk_ui_core.h`, not part of the SDK): a row among the rows of a stock menu (SETTINGS), with its icon. `core/digikit-menu` and `test/ui-demo` use it.
- The one firmware hook of the module is the end of the SETTINGS item list rebuild (`cf.menu.hook.settings_list.tail`, `src/shim.s` `dk_hook`). It calls the optional symbol `dk_app_settings_rows(settings_menu)` (`include/dk_ui_core.h`) that an app implements: patchkit resolves it to 0 when no app is in the image, and the hook then does nothing. `core/digikit-menu` and `test/ui-demo` implement it (they conflict: one DIGIKIT row). The layers are in [docs/ARCHITECTURE.md](../../../../docs/ARCHITECTURE.md).
- `dk_ui_tick(ui, fn, arg, period_ms)` is a table of four timers (`src/tick.h`, no firmware, tested in `host/tests/tick.rs`) behind one function on the runtime's `dkrt_poll_add` list; it is on the list only while a timer runs. The block clock times the periods (the UI-loop pass when there is none).
- An empty list paints the same inverted title bar as a list with rows, then its `empty` text. A text page opens at its first row; a menu or list keeps its selected row.

Proof and demo: `tests/fixtures/modules/ui-demo`, driven by `uv run python -m tools.patchkit.pk_uidemo shots|leak --device dt2|dn2`.
Details: [docs/findings/71-ui-components-1-17.md](../../../../docs/findings/71-ui-components-1-17.md).

The font of a plain menu or text page is read from the operand of one instruction of the firmware's VerticalMenuView constructor
(`cf.menu.site.vertical_menu_view_ctor.font_ref`), not from a fixed offset in the view: the class layout differs between firmwares.

## Module screens, SETTINGS rows and text input

Written for the track manager (docs/plans/track-manager.md). `dk_ui_open(ctx, &ui)` gives each module its own handle (its owner id from `dkrt_ctx_owner`, plus one);
core/ui registers one unload observer with the runtime (`dkrt_unload_observer_add`) and, when a module is unloaded, closes its views, removes its SETTINGS rows and its
`dk_ui_tick` timers and drops its pending text input callback.

- `dk_ui_settings_row(ui, text, open, arg)` (init only): `src/settings_rows.{h,c}`, at most 4 rows in load order. A stock row holds a key, the module's name and the row's number in the module, and runs the row that has that key now: rows survive a reload (the old stock row runs the reloaded module's row), and after a plain unload the stale row stays in the list built by the firmware and does nothing. A module loaded live for the first time after SETTINGS was built gets its row at the firmware's next rebuild of the list. `dk_hook` (shim.s) calls the app's `dk_app_settings_rows`, then `ui_settings_rows_tail`, which adds the registered rows with `dk_ui_stock_row`. A stock row finds its entry by id.
- `dk_ui_view_open/close`: `src/view_pool.{h,c}`, a pool of 4. The pool holds no firmware: `ui_view_fw` (push, close, invalidate) is implemented in ui.c with the stock View: a make_shared block (the control-block vtable of the stock ConfirmWindow, whose dispose calls slot 0), View::ctor, a copy of the View vtable with slot 2 (key) and 4 (draw) replaced, the opaque byte (1 full screen, 0 overlay) set before the push, our reference released after it. The firmware draws only after an invalidate, so the pool asks for a redraw of every open view on each UI pass while any is open. Keys come through the device's key map (`src/key_map.c`, `dkrt_dev.key_codes` reversed): an overlay passes every key on, a full-screen view takes the keys it does not pass on itself.
- `dk_canvas` (`sdk/include/dk/platform/canvas.h`): `src/canvas_map.{h,c}` turns the top-left calls into the firmware's (text y and rectangle rows count from the screen bottom; the stock paint code draws its title bar at rows 0x39 to 0x3f) and clips; a frame is four clipped lines. `text_in` and `text_width_in` take a `dk_font`: the stock font (box height 6) or the large ROM font (box height 15, anchor `cf.text.data.font_large`, optional: a port without it draws the small font).
- `dk_ui_text_input`: `src/text_input.{h,c}` checks the desc (`max_len` 1 to 15 and within the screen's 16); ui.c builds the stock NameView (8-argument constructor, no number) with `done` as the `std::function<void(std::string, int)>` the firmware calls on YES. One input at a time (`DK_E_BUSY`).
- Host tests: `host/tests/{settings_rows,views,canvas,text_input,keys,tick}.rs`; device test: `tests/fixtures/modules/ui-views/tests/live/`.
- SETTINGS row keys (module name, row number) are kept for the session, at most 8 distinct pairs.
