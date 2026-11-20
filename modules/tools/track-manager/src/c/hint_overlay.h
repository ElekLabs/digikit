/* hint_overlay: a strip over the stock screen with the number and name of the active track, while the shortcut is held. */
#ifndef HINT_OVERLAY_H
#define HINT_OVERLAY_H
#include <dk/platform/ui.h>

#include "track_names.h"

/* Keeps the UI handle and the names (both must outlive the module). */
void hint_overlay_init(dk_ui *ui, const track_names *names);

/* The shortcut's `on` and `off`: open and close the overlay. */
void hint_overlay_show(void *arg);
void hint_overlay_hide(void *arg);

#endif
