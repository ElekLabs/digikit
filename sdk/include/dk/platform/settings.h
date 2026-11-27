/* <dk/platform/settings.h>: a module's own persistent values (the imports dk_settings_open, dk_settings_get, dk_settings_put and dk_settings_remove).
   The values are kept in digikit's settings record on the card (8 KiB shared by all modules and the runtime; a value is 0 to 1024 bytes).

     init:  r = dk_settings_open(ctx, &settings);                    the same handle every time; DK_E_NOT_FOUND outside init
            n = dk_settings_get(settings, "volume", &v, 1);          n = 1: the value saved by an earlier boot; DK_E_SET_NOKEY: none yet
     later: dk_settings_put(settings, "volume", &v, 1);               in the UI task (tick, a menu callback)

   The runtime stores the key as "<module name>/<key>". A module reaches its own key space only: the module name and the key together are at most
   47 printable ASCII characters (no space), the key is not empty, does not hold "/" and does not hold "..". A key that breaks a rule is
   refused with E_SET_KEY and nothing is written. The prefix stops mistakes, not hostile code: modules are native code with no memory protection.
   The module owns the layout of its values and their version: write a version byte first, and read older values or reset them.

   Writes reach the card about 2 s after the last change (and before a restart). The runtime never interprets a value.

   Task rules: get works in init and in the UI task. put and remove are for the UI task only (init, tick or a menu callback), never from a
   host command handler (MIDI task) or an interrupt: the record is one buffer. The runtime does not check the task.
   Results: get returns the length of the value (0 is valid), E_SET_NOKEY when the key has no value, E_SET_VALUE when `cap` is too small (nothing is
   written to `out`); put and remove return 0 (remove also when the key was not there); all can return E_SET_KEY (stale handle or bad key) and
   E_SET_OFF (the settings store is off on this card); put can return E_SET_VALUE (over 1024 bytes) and E_SET_FULL (the 8 KiB record is full). */
#ifndef DK_SETTINGS_H
#define DK_SETTINGS_H
#include <dk/core/module.h>
#include <dk/core/status.h>

/* The runtime's E_SET_* results (sdk/sdk.toml [protocol.status]) named in the comment above. */
#define DK_SET_E_KEY DK_E_SET_KEY     /* E_SET_KEY: stale handle or a key that breaks the rules */
#define DK_SET_E_FULL DK_E_SET_FULL   /* E_SET_FULL */
#define DK_SET_E_NOKEY DK_E_SET_NOKEY /* E_SET_NOKEY */
#define DK_SET_E_VALUE DK_E_SET_VALUE /* E_SET_VALUE */
#define DK_SET_E_OFF DK_E_SET_OFF     /* E_SET_OFF */

#endif
