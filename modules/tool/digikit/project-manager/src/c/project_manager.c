/* digikit/project-manager: a read-only screen of the current project, in SETTINGS. `init` adds the row PROJECT (core/ui). The calls
 * dk_project_current and dk_project_status make the project service a hard need of the module. */

#include <dk/core/module.h>
#include <dk/platform/ui.h>

#include "project_screen.h"
#include "workbench_queries.h"

#define SETTINGS_ROW_TEXT "PROJECT"

static int32_t init(dk_module_ctx *ctx)
{
    dk_ui *ui = 0;
    int32_t r = dk_ui_open(ctx, &ui);
    if (r != DK_OK)
        return r;
    project_screen_init(ui);
    /* The runtime takes queries only from a module that registered a settings descriptor, which this module has none of: the refusal leaves the
     * row working and the page without answers until the runtime accepts a module with queries alone. */
    (void)dk_workbench_register_queries(ctx, dk_workbench_queries, DK_WORKBENCH_QUERY_COUNT);
    return dk_ui_settings_row(ui, SETTINGS_ROW_TEXT, project_screen_open, 0);
}

/* The runtime closes the module's views when it unloads. */
static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
}

const dk_module_def dk_module = {.init = init, .fini = fini};
