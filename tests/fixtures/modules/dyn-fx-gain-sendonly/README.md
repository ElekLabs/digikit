# FX gain sendonly, run time (test)

The same effect as `test/dyn-fx-gain` with `route = "send"`, `routes = ["send"]` in its `DK_FX` record (`src/module.c`): it runs on the send route only (the FX ROUTES page says `SEND ONLY`). Used by `tests/regression/runtime/fx-routes-panel.test.ts` and `tests/regression/runtime/fx-routes-persist.test.ts` on both devices.
