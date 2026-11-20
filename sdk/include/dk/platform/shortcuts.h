/* <dk/platform/shortcuts.h>: key combinations a module binds (dk_shortcut_add). Hand-written.

   A shortcut is two logical keys: hold keys[0], press keys[1]. The press of keys[1] is not passed to the stock views, so a combination such as [TRK]+[YES]
   does not also act as YES; keys[0] is never taken (the stock firmware needs it). `on(arg)` runs when the combination is complete and `off(arg)` when either key
   is released; the keys, their repeats and their releases are swallowed while it is active, every other key (a TRIG key under [TRK]) passes as usual.

   Keys are logical (<dk/gen/keys.h>: DK_KEY_TRK, DK_KEY_YES, DK_KEY_TRIG(n)); a module holds no panel code. A key the device lacks makes the shortcut inactive, so a
   module need not check the device. Call dk_shortcut_add in `init`; the shortcuts go away with the module. The first module to bind two keys keeps the binding: a later
   module that binds the same pair loads with its shortcut disabled (the runtime logs SHORTCUT_CLASH).

   `on` and `off` run in the UI task, in the key-dispatch of the stock firmware: short, no waiting. A module that draws while the combination is held opens its view in `on`
   and closes it in `off`. */
#ifndef DK_PLATFORM_SHORTCUTS_H
#define DK_PLATFORM_SHORTCUTS_H
#include <dk/core/module.h>
#include <dk/gen/keys.h>

typedef struct dk_shortcut_def dk_shortcut_def; /* also declared by <dk/gen/imports.h> */
struct dk_shortcut_def {
    const char *
        id; /* unique in the module, at most 15 characters of letters, digits, _, - and . (for a later list and rebinding) */
    const char *
        label; /* at most 23 characters, space to tilde (for a later list and help page), e.g. "TRACK NAME HINT" */
    dk_key keys
        [2]; /* default binding: hold keys[0], press keys[1]; neither is DK_KEY_NONE, they differ */
    void (*on)(void *arg);  /* required: the combination is complete (keys[1] was swallowed) */
    void (*off)(void *arg); /* optional (NULL: none): either key was released */
    void *arg;              /* passed to both callbacks */
};

/* int32_t dk_shortcut_add(dk_module_ctx *ctx, const dk_shortcut_def *def): in <dk/gen/imports.h>. A module has at most 4 shortcuts. */
#endif
