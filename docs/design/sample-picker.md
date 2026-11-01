# On-device sample picker (slice 2)

Status: accepted by Em on 2026-11-01 with the recommendations in section 5; refusals show in the track row, not in a dialog.
Sources: sdk/include/dk/{ui,files,sample_track}.h, docs/design/sample-api-1.5.md, docs/MODULE-AUTHORING.md, modules/core/runtime/src/rs/src/tracks.rs, modules/core/apps/digikit-menu/README.md, finding 71, plan "Picker design direction".

## 0. Facts that shape the design

- OLED: 128 x 64 px, 1 bit (framebuffer 1024 bytes; PNGs are 4x). A DK_UI_LIST has an inverted title bar at the top (about 8 px) and 7 px rows, so about 7 rows and about 21 characters per row in the firmware font **[O: count rows and columns from the first dk:dev shot]**. Row text is cut at 23 characters (DK_UI_ROW 24). A toast is hidden while a menu is open, so no toast inside the picker.
- Keys in a list: UP/DOWN move, YES runs the row, RIGHT opens the side panel, LEFT/NO closes it, NO on the list calls `desc.back` (return 1 = stay open).
- `sample_select` copies the path and returns at once (0 or an error); the load runs in the runtime UI task. Errors that happen later are sticky in `sample_track_status` until the next select/unbind.
- DT2 has a sample backing (`core/sample-pool`, DT2 only). DN2 has none yet: `sample_select` returns E_CONTRACT.
- Handle problem (section 3): `sample_tracks(local_id)` only finds a table the calling module declared. A picker has no table.
- Gap: a content module has no UI timer. It cannot refresh a screen while a load runs (section 3, G4).

## 1. Entry and track capture

Entry: the module's settings menu. SHOT exports `DK_MENU(shot_menu)`; path on the device is SETTINGS > DIGIKIT > MODULES > (SHOT) RIGHT > SETTINGS... > `SAMPLES...`. A shortcut on the SHOT page is not possible: `dk/ui.h` has no hook into a machine page, and the UI layer cannot learn the sequencer's selected track. So the track is chosen explicitly, as the plan says.

Capture: the track list (screen 1) is the first screen. YES on track row n stores n in a static `g_pick_track` (0..15) and opens the browser. The browser never changes `g_pick_track`. Browsing changes nothing on the track. Only the select action calls `sample_select`. Closing with NO leaves the old sample playing.

Recommendation for the code location (answers "UI module"): build the picker as a shared C source in the SDK (`sdk/src/sample_picker.c` plus `sdk/include/dk/sample_picker.h`), compiled into SHOT (and any later sampler) and entered from its `dkm_menu`. Reason: SHOT owns the table and the handle, so the picker needs no new runtime call. A stand-alone UI-layer module needs the API additions in G1 and G2. If Em wants the stand-alone module, build G1 first. The picker source does not change either way: it takes `(api, handle)`.

## 2. Screens and keys

Common: font and views are the firmware's (core/ui). One button: YES on a file selects it and returns to the track list. No confirm before select (select supersedes a running load, so a wrong pick is one more pick). The only dialogs are for immediate refusals (see 2.4).

### 2.1 Screen 1: track list (DK_UI_LIST, title `SAMPLES`)

```
+----------------------+   128 x 64 px, ~21 characters
|       SAMPLES        |   title bar (inverted)
| 01 KICK808    S 1.2s |   row = track, name, mono/stereo, length
| 02 SNARE_A    M 0.4s |
| 03 LOADING...        |
| 04 -                 |   unused track
| 05 !POOL FULL        |   sticky error
| 06 -                 |
| 07 -                 |
+----------------------+   (16 rows, list scrolls; FREE row is last: FREE 3.2M FR)
```

Rows: 16 tracks, then one text row `FREE ...` (needs G3; without G3 the row is left out).
Row text: `NN NAME` (name 10 characters, a longer name keeps its start and end around `~` like core/digikit-menu) and a right-aligned tail: `S 1.2s` / `M 0.4s` from `sample_track_info` (READY), `LOADING...` / `QUEUED` (state), `!` plus the error text (2.4), or `-` (UNUSED).
Keys: YES = open the browser for this track (screen 2). RIGHT = side panel: `CLEAR` (calls `sample_unbind`, one press, no confirm; the old notes stop, row goes to `-`), `INFO` (screen 4). NO = back to SETTINGS.
Refresh: `dk_ui_refresh` after any action and, while a track is QUEUED/LOADING, on the UI tick (G4).
Names: after a project change the runtime reloads the selections (api doc section 3). The row needs the file name, which the picker cannot know. Needs G2. Without G2 the row shows `NN <frames>` only and the name is blank after a project change.

### 2.2 Screen 2: folder browser (DK_UI_LIST, title `SAMPLES`)

```
+----------------------+
|       SAMPLES        |
| ..  /DRUMS/KICKS     |   row 0: parent folder, shows the current path
| > 808                |   folder (">" prefix)
| > ACOUSTIC           |
|   KICK_01       92K  |   file, size right aligned
|   KICK_02       88K  |
|   KICK_LONG_N~_FINAL |   long name: start ~ end
|   BAD NAME TOO LONG  |   disabled: path over the provider limit
+----------------------+
```

Source: `files_list(client, DK_FILES_SAMPLES, dir, notify, ctx)` (DT2 +Drive, DN2 store `samples/`). `files_client()` is acquired once in the picker's init call. The picker owns one request at a time; a new folder cancels the old ticket (`files_cancel`) and requests the new one.
Reading: `notify` runs in the UI task and only polls. It drains `files_poll` until it returns 0 (pending) or 2 (end), copies each row into the picker's table, then calls `dk_ui_refresh`. Fill and action callbacks never read storage. While the ticket is pending, fill adds one text row `READING...`; at end with no rows, `desc.empty` = `NO SAMPLES HERE`. A negative poll result shows one text row `LIST ERROR` plus the code and `NO` still works.
Table: up to 128 rows, each: kind, 4-byte size (if SIZE_VALID), and the leaf name (47 bytes). The full path is the current directory string plus the leaf, built when needed (limit 254 DT2, 151 DN2 sample paths; a longer one is a disabled row). Row identity is the full path, never a row index or the truncated display name (plan rule). More than 128 rows: the first 128 by sort order and a last text row `+N MORE`; the workbench can arrange folders. **[O]** ask Em if 128 is enough.
Sort: at end of listing, sort rows folders first, then names, case-insensitive, by an index array (insertion sort, 128 rows). The provider order is not promised, so the picker sorts. A change of the folder while the user is on row k keeps no selection.
Paging: core/ui scrolls the list. "Paging" is only the 128-row window above; no network of continuation pages is needed.
Keys: UP/DOWN move. YES on a folder = enter it (relist). YES on `..` = parent. YES on a file = select (2.3). NO = parent folder; at the root NO closes the browser (`back` returns 1 when it went up, 0 at the root). RIGHT on a file = side panel: `SELECT` (same as YES), `INFO` (screen 4 with the listing facts). LEFT/NO closes the panel.

Preview of size/channels/length: the listing gives size and hash only. A module cannot read the header (`store_read` reads the runtime store, not +Drive, and a sample is not in it). So the row shows size (K/M). Channels and length appear after the load (READY, `sample_track_info`) in screen 1 and INFO. A header preflight in the picker needs a new read call and is not worth it: the runtime already checks the header in `select`'s preflight and reports FORMAT. **Recommendation: no preview in v1.**

### 2.3 Select and load progress

YES on file F with track t: the picker builds the path, calls `sample_select(handle, t, path)`.
- Returns 0: close the browser (`dk_ui_close`), refresh screen 1. Row t reads `QUEUED`, then `LOADING...`, then `S 1.2s`. READY is the end: nothing more to press.
- Returns an error: stay in the browser and show the refusal dialog (2.4). Old binding is unchanged (preflight fails before any backing change, api doc 2.1).
The track is never reported READY by the picker itself; only `sample_track_status == 3` sets the row (plan rule: do not report READY early).
Supersede: another YES on the same track while QUEUED/LOADING is allowed; the runtime handles it.

### 2.4 Errors

Async errors (row tail on screen 1, sticky until select/unbind) and INFO text (screen 4):

| Code | Row tail (max 14) | INFO text |
| --- | --- | --- |
| E_NOT_FOUND -1 | `!MISSING` | FILE NOT FOUND |
| E_READ -3 | `!READ ERROR` | CARD READ FAILED |
| E_FORMAT -5 | `!BAD FORMAT` | NEEDS PCM16 48K MONO OR STEREO |
| E_TOO_BIG -13 | `!TOO BIG` | DOES NOT FIT THE POOL |
| E_LOST -15 | `!LOST` | BACKING LOST, RELOAD FAILED |
| E_POOL -16 | `!POOL FULL` | POOL FULL: CLEAR A TRACK |
| E_CHANGED -17 | `!CHANGED` | FILE CHANGED SINCE SELECTION |
| E_SEL_PATH -40 | `!BAD PATH` | PATH TOO LONG OR BAD |
| E_CONTRACT -48 | `!NO POOL` | NO SAMPLE POOL ON THIS DEVICE |
| other | `!ERR -nn` | (code) |

Immediate refusal from `sample_select`: no dialog (Em, 2026-11-01: one-button flows, no confirm that ignores its answer). The browser closes and the track row on screen 1 shows the short error text from the table above (e.g. `POOL FULL`, `MISSING`), with the old binding unchanged. On DN2 E_CONTRACT shows `NO SAMPLE POOL` in the row until the DN2 pool exists. With G3 the picker knows at open and screen 1 shows one text row `NO SAMPLE POOL ON THIS DEVICE` before the user picks.
E_CHANGED fix: YES on the same file again (re-selects, new snapshot). E_LOST on DT2 after a stock reset: the runtime reloads; the row passes LOADING again. No auto-retry beyond that (api doc question 6).

### 2.5 Screen 3: clear (side panel CLEAR)

One press, no confirm: `sample_unbind`. Row goes to `-`. The machine on that track plays silence at the next note (api doc). Nothing else is changed; Elektron project files are not touched (keep-stock rule): all bindings live in the digikit store.

### 2.6 Screen 4: INFO (DK_UI_TEXT_PAGE, title = `TRACK 03`)

```
+----------------------+
|       TRACK 03       |
| KICK808              |
| /DRUMS/KICKS/KICK808 |   path (scrolls)
| STEREO 48000 HZ      |
| 57600 FRAMES  1.2 S  |
| STATE READY          |
| POOL FREE 3.2M FR    |   with G3
+----------------------+
```

Error state: the line `STATE` shows the INFO text from 2.4. Needs G2 for the name/path lines (without G2: those two lines are `-`).

## 3. API 1.5 calls, callbacks and gaps

| Call | From | Purpose |
| --- | --- | --- |
| `files_client` | picker init (called from `dkm_init` of the host module) | listing client, positive handle |
| `files_list`, `files_cancel` | action callbacks (folder enter, parent), `back` | start/replace a listing |
| `files_poll` | files notify callback (UI task) | copy rows, then `dk_ui_refresh` |
| `sample_select(h,t,path)` | file row action | bind, section 2.3 |
| `sample_unbind(h,t)` | CLEAR action | section 2.5 |
| `sample_track_status(h,t)`, `sample_track_info(h,t,out)` | fill of screen 1 and 4 | state, frames/rate/channels |

Check the Api `size` covers `sample_track_info` and `files_poll` before use (as SHOT does).

Handle source. The runtime does not check ownership: `client(handle)` accepts any open client slot 1..CLIENTS, so a picker that guesses `handle = 1` works by accident and breaks as soon as two modules have tables or the load order changes. Not acceptable. Clean sources:
- (chosen for slice 2) the host machine passes its own handle: SHOT stores it in `dkm_init` already (`static uint32_t tracks`); its `dkm_menu` calls `dk_picker_open(m, api, tracks)`.
- (stand-alone picker, flagged) **G1: the API lacks a way to find another module's table.** Add `sample_tracks_find(index)` (0-based over open tables, returns a handle or `E_NOT_FOUND`) plus `sample_tracks_owner(handle, out_id, cap)` for the title, and make `select/unbind` check that a handle is valid for the call, not only open. Runtime change in tracks.rs; minor API 1.6.

Other gaps (all additive, none blocks the host-menu route except G4 for live progress):
- **G2** (built, API 1.6, sample-api-1.5.md section 6) `sample_track_path(h, track, buf, cap)`: the stored path of a track, so screen 1 shows names after a project change. The runtime keeps the path already (remembered for reload).
- **G3** `sample_pool_info(h, out)`: free frames and whether a backing exists (E_CONTRACT = none). `dkpool_free_frames` exists inside the runtime; design question 4 recommended exposing it read-only. This gives the "what fits" text and the DN2 notice up front.
- **G4** (built, section 6.3) a UI tick for content modules: `dk_ui_tick(fn, ctx, period_ms)` in `dk/ui.h`, implemented by core/ui with the runtime's `dkrt_poll_set` (digikit HEALTH does this already; content modules cannot import `dkrt_*`). `dl_set_tick_cb` is a block interrupt and must not call UI. Without G4 the load state on screen 1 refreshes only on key presses: the user would see `QUEUED` until the next key. Workaround for a first test only: refresh from the files notify callback.
- **G5** (built, section 6.2) path form: DT2 `sample_select` takes an absolute +Drive path, DN2 a `samples/...` path (MODULE-AUTHORING). The listing gives root-relative paths. The picker must not know the prefix. Recommend `sample_select` accepts the root-relative path of a `DK_FILES_SAMPLES` row and the runtime adds the prefix. **[O]** check what `select` does today with a root-relative path; if it refuses, the runtime adds the prefix in tracks.rs (small change).

## 4. Module layout, limits, tests

Files:
- `sdk/include/dk/sample_picker.h`: `dk_picker_open(dk_ui_menu *m, const dk_api *api, uint32_t handle)`.
- `sdk/src/sample_picker.c`: screens, callbacks, statics.
- `sdk/src/picker_logic.h` and `sdk/src/picker_logic.c`: pure functions, no API calls: sort order, leaf-name truncation, path join/parent, size/length text, error text for a code, row text for track state.
- `modules/machines/shot/src/module.c`: `DK_MENU(shot_menu)` and the `[[object]]` sources list gets `sdk/src/sample_picker.c`. `requires = ["core/ui"]` is added to the manifest (imports `dk_ui_*`); the module still loads without core/ui (calls return failure values; then the entry row is simply absent).
- No new manifest for a picker module in this route.

Limits: code about 4 to 6 KB; statics about 8 KB (128 rows x 56 bytes, 16 track contexts, path buffers 256 bytes x 2). Menus are never freed: build each menu once (track list, browser, INFO page, panels), kept in statics, rebuilt by `dk_ui_refresh`. All ctx and descriptors in module memory. Rows 23 characters. One browser instance (not re-entrant): a second open reuses it.

Tests:
1. Host unit tests of `picker_logic` and the screen logic against a fake `dk_api` and the host fake of core/ui (same pattern as `modules/core/adapters/ui/host/tests/menu.rs`, which compiles a C source against a fake runtime): sort order (folders first, case-insensitive), 128-row cap and `+N MORE`, long-name truncation, path join and parent at the root and at the 254/151 limit, error-code to text mapping for every code of 2.4, track-row text per state. No test of third-party behaviour or argv.
2. `dk:dev` screenshot sheet on DN2 (SHOT change reloads by boot, DN2 5 s; a pure `core/ui` change swaps in about 4 s). Per screen a `shot:`:
   `mise run dk:dev -- digikit/shot --device dn2 --store 'digikit/shot*' --keys 'tap:NO,tap:SETTINGS,wait:400,<tap steps to DIGIKIT > MODULES > RIGHT > SETTINGS...>,shot:tracks,tap:YES,wait:800,shot:browse,tap:DOWN*2,tap:RIGHT,shot:panel,tap:LEFT,tap:YES,wait:600,shot:refused,tap:YES,tap:NO,tap:DOWN,tap:RIGHT,tap:DOWN,tap:YES,shot:info'`
   (step lists are tuned when the screens exist). Shots: `tracks`, `browse`, `panel`, `refused` (DN2: NO SAMPLE POOL), `info`, `empty`, `error` (needs a DT2 run with a bad file). Check DT2 at the end with the same script. Look at every PNG before handoff.
3. One DT2 runtime test, `shot-picker.test.ts` in tests/regression/runtime (not written yet), built on the helpers of `sample-playback.test.ts`: put a stereo PCM16 file in +Drive, drive the keys to select it on track 1 through the picker, wait for the `dk_strack` record of track 1 to show `ready == 1` (observe the runtime record and `sample_track_status`, not the framebuffer), then play the note and check the MOD_DSP event of SHOT's own index as that test does. Also CLEAR: record `ready == 0`. One device, one pass; no all-cell sweep.
4. Gate: `mise run module:check digikit/shot`, `mise run test:host`, then the owner's runtime test above. No gate run until the end of the slice.

## 5. Open questions for Em (with recommendations)

1. Picker inside each sampler's `dkm_menu` (shared source) or a stand-alone module? Recommend the shared source now; stand-alone later with G1.
2. Add G2 (track path), G3 (pool info), G4 (UI tick) to the runtime/ui as API 1.6 and ui additions before the picker? Recommend yes for G4 and G2 (without them the screen is blind during loads and after a project change); G3 can follow, DN2 gets the E_CONTRACT dialog meanwhile.
3. No header preview in v1 (size only). Recommend accept.
4. 128 rows per folder, picker sorts. Recommend accept; add paging only if a real card needs it.
5. Errors on an immediate refusal: decided (Em, 2026-11-01): no dialog; the track row shows the error text (section 2.4).
6. DN2: picker shows `NO SAMPLE POOL` until the DN2 allocator exists. Recommend ship the picker on DT2 first and keep DN2 as the screenshot-iteration device.
7. G5: should the runtime add the root prefix to `sample_select`? Recommend yes (keeps the picker device-agnostic).

## 5b. As built (slice 2, DN2 screens)

- Two calls plus the fini: `dk_picker_init(api, handle)` in `dkm_init` (the listing client is init-only), `dk_picker_open(m)` in the settings menu fill, `dk_picker_fini()`.
- The 128-row table keeps the first 128 rows in sort order while the listing arrives (insertion by index), so the cap does not depend on the provider order. A name over 47 bytes is not shown and counts in `+N MORE`.
- A path over 255 bytes is refused by the picker itself (`!BAD PATH` in the track row); there are no disabled rows, because the picker does not know the device limit.
- The lists use a `check` callback and take the mark off at once. A list without it is closed by the stock handler after the row action, and a list that opens another view is not on the stack when that view closes (seen on the DN2 screen). The picker closes the browser itself and opens the track list again.
- INFO text pages show about 16 characters per row; list rows about 21 to 23. Row text in lists is cut at 23 (worst-case wide names fit).
- Title of INFO is `INFO`; the first row is `TRACK n`.

## 6. Estimate and risk (agent time)

- Picker logic and screens, host tests: 4 to 6 hours.
- G2 + G4 (+ G3): 3 to 5 hours (runtime tracks.rs, core/ui, sdk.toml, `patchkit gen`); G1 and G5 add 2 hours.
- dk:dev screens on DN2, DT2 check, regression test: 3 to 4 hours.
Total: 1.5 to 2 days of agent time. Outcome risk: medium. The main risks are (a) the list view with many or zero rows in core/ui (an empty list faulted before; finding 71), (b) row count and width differ from the mockups (measure first), (c) DT2 +Drive listing latency and the 254-byte path edge, (d) no UI tick until G4 lands. Payoff is high (the first release needs on-device selection); confidence is good for the host-menu route, lower for the stand-alone module.
