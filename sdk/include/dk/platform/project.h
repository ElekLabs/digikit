/* <dk/platform/project.h>: the project list and the project summary of the `project` service (dk_project_list and dk_project_get, <dk/gen/imports.h>), for a module that
 * browses the firmware's projects, such as the Project Manager. Both are read-only and run in the UI task; a module that calls them needs the `project` service.
 *
 *   dk_project_entry e[16];
 *   int32_t n = dk_project_list(0, e, 16);              // the first page; the next one starts at e[n - 1].slot + 1
 *
 *   dk_project_summary s;
 *   int32_t r = dk_project_get(slot, &s);               // 0, DK_E_AGAIN (asked: call again in a later pass) or DK_E_NOT_FOUND
 *
 * The layouts are explicit: the padding is a field, and the service checks the same sizes. */
#ifndef DK_PLATFORM_PROJECT_H
#define DK_PLATFORM_PROJECT_H

#include <dk/core/status.h>
#include <dk/gen/imports.h>
#include <stddef.h>
#include <stdint.h>

/* dk_project_entry.flags */
#define DK_PROJECT_CURRENT 1u      /* the project is the current one */
#define DK_PROJECT_DATA 2u         /* a `.dkp` file of the slot is in the store */
#define DK_PROJECT_DATA_UNKNOWN 4u /* bit 1 is not known yet: the first listing of the store folder has not ended */

/* One project of the firmware's list. */
typedef struct dk_project_entry {
    uint8_t slot; /* 0 to 127 */
    uint8_t flags;
    uint8_t name_len;
    uint8_t name[16]; /* as the firmware lists it, zero padded */
} dk_project_entry;
_Static_assert(sizeof(dk_project_entry) == 19, "dk_project_entry layout");
_Static_assert(offsetof(dk_project_entry, name) == 3, "dk_project_entry.name");

/* dk_project_module.state */
#define DK_PROJECT_MODULE_UNKNOWN 0u /* the card was not looked at */
#define DK_PROJECT_MODULE_LOADED 1u  /* the module is loaded now */

/* The most modules of a project's list (the runtime's module limit) and the longest module id. */
#define DK_PROJECT_MODULES_MAX 8
#define DK_PROJECT_ID_MAX 31

/* One module of the project's module list. */
typedef struct dk_project_module {
    uint8_t id_len;
    uint8_t id[DK_PROJECT_ID_MAX]; /* `<author>/<name>`, not NUL terminated */
    uint32_t hash;                 /* the store hash of the module file at the project's last save */
    uint8_t settings;              /* the settings entries the project holds for the module (at most 255) */
    uint8_t state;
    uint8_t pad[2];
} dk_project_module;
_Static_assert(sizeof(dk_project_module) == 40, "dk_project_module layout");
_Static_assert(offsetof(dk_project_module, hash) == 32, "dk_project_module.hash");
_Static_assert(offsetof(dk_project_module, settings) == 36, "dk_project_module.settings");

/* The header and the module list of one project. */
typedef struct dk_project_summary {
    uint8_t slot;
    uint8_t name_len;
    uint8_t name[16];
    uint8_t pad[2];
    uint32_t stamp;   /* the stock save stamp of the file */
    uint32_t counter; /* the write counter of the file, 0 for a project with no file */
    uint8_t module_count;
    uint8_t pad2[3];
    dk_project_module modules[DK_PROJECT_MODULES_MAX];
} dk_project_summary;
_Static_assert(sizeof(dk_project_summary) == 32 + 40 * DK_PROJECT_MODULES_MAX, "dk_project_summary layout");
_Static_assert(offsetof(dk_project_summary, stamp) == 20, "dk_project_summary.stamp");
_Static_assert(offsetof(dk_project_summary, module_count) == 28, "dk_project_summary.module_count");
_Static_assert(offsetof(dk_project_summary, modules) == 32, "dk_project_summary.modules");

#endif
