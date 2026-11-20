/* manager_screen: the full-screen track manager: a grid of the tracks, rename with the stock naming screen. */
#ifndef MANAGER_SCREEN_H
#define MANAGER_SCREEN_H
#include <dk/core/module.h>
#include <dk/platform/ui.h>

#include "track_names.h"

/* Keeps the UI handle, the names and the settings handle (all must outlive the module). `names` is saved under the key "names" after a rename. */
void manager_screen_init(dk_ui *ui, track_names *names, dk_settings *settings);

/* Opens the screen on the active track (the SETTINGS row's `open`). */
void manager_screen_open(void *arg);

#endif
