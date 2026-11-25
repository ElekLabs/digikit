# FX gain send, run time (test)

The same effect as `test/dyn-fx-gain` with `route = "send"` in its `DK_FX` record (`src/module.c`): it starts on the send route and may be switched to the master (`routes` defaults to `["send", "master"]`). Used by `runtime/platform/fx/tests/fx-routing.device.test.ts` and `runtime/platform/fx/tests/fx-routes-persist.device.test.ts` on both devices.
