/* project_screen: the read-only project screen (SETTINGS > PROJECT): the current project and its save state. */
#ifndef PROJECT_SCREEN_H
#define PROJECT_SCREEN_H
#include <dk/core/module.h>
#include <dk/platform/ui.h>

/* Keeps the UI handle (it must outlive the module). */
void project_screen_init(dk_ui *ui);

/* Opens the screen (the SETTINGS row's `open`). */
void project_screen_open(void *arg);

#endif
