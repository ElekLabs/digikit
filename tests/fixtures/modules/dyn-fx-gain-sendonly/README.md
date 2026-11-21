# FX gain sendonly, run time (test)

The same effect as `test/dyn-fx-gain` with `route = "send"`, `routes = ["send"]` in its `DK_FX` record (`src/module.c`): it runs on the send route only (the FX ROUTES page says `SEND ONLY`). Used by `image-modules/points/fx/tests/fx-routes-panel.device.test.ts` and `image-modules/points/fx/tests/fx-routes-persist.device.test.ts` on both devices.
