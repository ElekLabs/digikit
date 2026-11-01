# FX gain send, run time (test)

The same effect as `test/dyn-fx-gain` with `route = "send"` in its `DK_FX` record (`src/module.c`): it starts on the send route and may be switched to the master (`routes` defaults to `["send", "master"]`). Used by `tests/regression/runtime/fx-routes-panel.test.ts` and `tests/regression/runtime/fx-routes-persist.test.ts` on both devices.
