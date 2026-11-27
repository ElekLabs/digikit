/* The handlers of the queries in workbench/project_manager.proto: they call the project service (dk_project_list, dk_project_get) and map the results
 * with project_query. Both run in the UI task. */
#include <dk/core/module.h>
#include <dk/platform/project.h>

#include "project_query.h"
#include "workbench_queries.h"

#define PAGE_SIZE 16

int32_t project_manager_list(dk_module_ctx *ctx, const project_manager_ListRequest *request,
                             project_manager_ListReply *reply)
{
    (void)ctx;
    dk_project_entry entries[PAGE_SIZE];
    int32_t n = dk_project_list(request->first_slot, entries, PAGE_SIZE);
    if (n < 0)
        return n;
    for (int32_t i = 0; i < n; i++) {
        project_manager_Project *item = &reply->items[i];
        item->slot = entries[i].slot;
        item->name_len = project_query_text(item->name, sizeof item->name, entries[i].name,
                                            entries[i].name_len < 16 ? entries[i].name_len : 16);
        project_query_flags(entries[i].flags, &item->current, &item->has_data,
                            &item->data_unknown);
    }
    reply->items_count = (uint32_t)n;
    reply->next = project_query_next((uint32_t)n, PAGE_SIZE, n ? entries[n - 1].slot : 0);
    return 0;
}

int32_t project_manager_get(dk_module_ctx *ctx, const project_manager_GetRequest *request,
                            project_manager_GetReply *reply)
{
    (void)ctx;
    dk_project_summary summary;
    int32_t r = dk_project_get(request->slot, &summary);
    r = project_query_get_outcome(r, &reply->loading, &reply->found);
    if (r != 0 || !reply->found)
        return r;
    reply->slot = summary.slot;
    reply->name_len = project_query_text(reply->name, sizeof reply->name, summary.name,
                                         summary.name_len < 16 ? summary.name_len : 16);
    reply->stamp = summary.stamp;
    reply->counter = summary.counter;
    uint32_t count = summary.module_count < DK_PROJECT_MODULES_MAX ? summary.module_count
                                                                   : DK_PROJECT_MODULES_MAX;
    for (uint32_t i = 0; i < count; i++) {
        const dk_project_module *m = &summary.modules[i];
        project_manager_Module *out = &reply->modules[i];
        out->id_len = project_query_text(out->id, sizeof out->id, m->id,
                                         m->id_len < DK_PROJECT_ID_MAX ? m->id_len : DK_PROJECT_ID_MAX);
        out->hash = m->hash;
        out->settings = m->settings;
        out->loaded = m->state == DK_PROJECT_MODULE_LOADED;
    }
    reply->modules_count = count;
    return 0;
}
