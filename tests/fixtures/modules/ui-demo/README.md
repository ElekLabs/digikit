# test/ui-demo

A content module (`.dkm`, loaded from the card) that adds the SETTINGS row **UI DEMO** with `dk_ui_settings_row` in `init`. The row opens a menu with one row of each kind of core/ui: a toggle, a value, a toast, a confirm, a choice, a text page, a list
(check marks, a side panel with two actions) and an empty list. It requires `core/ui`. The real DIGIKIT menu is the system package's, a separate row; this is a controls fixture.
Its command 0x0f reports the value, the check marks, the events and the last answer, so the device test reads them without a symbol.

From the repository root after [setup](../../../../docs/DEVELOPMENT.md#setup), with the DN2 firmware available:

```sh
mise run dk:ui-demo
```

This starts or reloads the kept DN2 emulator with zero DSP (no audio) and the module on its card, navigates to the demo and opens a local browser faceplate. The command prints the exact panel URL. Keep it running while using the panel; Ctrl-C stops the browser server and leaves the emulator alive. Stop the emulator with `mise run dk:dev -- emulator --store 'test/ui-demo*' --stop`. For the real menu, use `mise run dk:menu`; see [visible previews and module iteration](../../../../docs/DEVELOPMENT.md#visible-preview).

For screenshots without opening a browser, or to check DT2 (the demo row is the tenth of SETTINGS, after DIGIKIT):

```sh
mise run dk:dev -- emulator --store 'test/ui-demo*' --device dt2 --keys 'tap:NO,tap:SETTINGS,wait:400,tap:DOWN*9,tap:YES,wait:600,shot:menu'
pnpm nx:pilot run sdk-ui-zero-tests:test-device
```

The preview is for iteration; the named device target is the acceptance check. It runs this fixture on DT2 1.17 and DN2 1.12 with zero DSP and no audio session, then checks row edits, text scrolling, list checks and panel focus, back refusal, the empty list, confirm and choice answers and cancellation, and toast expiry. Callback state and OLED comparisons are recorded through the shared runtime harness.
