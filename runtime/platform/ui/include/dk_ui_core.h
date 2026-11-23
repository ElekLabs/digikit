/* dk_ui_core.h: calls of core/ui for other core modules only. Not part of the module SDK (sdk/include/dk/platform/ui.h). */
#ifndef DK_UI_CORE_H
#define DK_UI_CORE_H
#include <stdint.h>
#include <dk/platform/ui.h>

/* The UI handle for core modules, which have no dk_module_ctx to open it with: the same handle dk_ui_open gives a content module. */
dk_ui *dk_ui_core_handle(void);

/* A row among the rows of a stock firmware menu (SETTINGS), with the menu's icon. fn(arg, 0) runs on YES.
   Call it from a hook that rebuilds the stock menu; stock_menu is the menu object the hook gets. */
void dk_ui_stock_row(uint8_t *stock_menu, const char *text, dk_ui_action_fn *fn, void *arg);

/* The rows of the SETTINGS menu that the system package adds come through the `ui.settings_rows` hook of the runtime (dkrt_ui_settings_rows): core/ui
   calls it at the end of the SETTINGS item list rebuild, and the package's app adds its rows with dk_ui_stock_row. With no package, nothing is added. */

#endif
