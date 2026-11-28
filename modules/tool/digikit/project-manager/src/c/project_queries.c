/* The handlers of the queries in workbench/project_manager.proto: they call the project service (dk_project_list, dk_project_get, dk_project_settings, dk_project_read) and map the results
 * with project_query. Both run in the UI task. */
#include <dk/core/module.h>
#include <dk/platform/project.h>

#include "project_query.h"
#include "workbench_queries.h"

#define PAGE_SIZE 16
#define READ_PAGE 768   /* the `max_len` of ReadFileReply.data */
#define SETTINGS_PAGE 4 /* the `max_count` of GetSettingsReply.entries */

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
        project_query_flags(entries[i].flags, &item->current, &item->has_data, &item->data_unknown);
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
        out->id_len =
            project_query_text(out->id, sizeof out->id, m->id,
                               m->id_len < DK_PROJECT_ID_MAX ? m->id_len : DK_PROJECT_ID_MAX);
        out->hash = m->hash;
        out->settings = m->settings;
        out->loaded = m->state == DK_PROJECT_MODULE_LOADED;
    }
    reply->modules_count = count;
    return 0;
}

int32_t project_manager_get_settings(dk_module_ctx *ctx,
                                     const project_manager_GetSettingsRequest *request,
                                     project_manager_GetSettingsReply *reply)
{
    (void)ctx;
    dk_project_setting settings[SETTINGS_PAGE];
    uint32_t next = 0;
    int32_t r =
        dk_project_settings(request->slot, (const uint8_t *)request->module, request->module_len,
                            request->version, request->cursor, settings, SETTINGS_PAGE, &next);
    int32_t outcome =
        project_query_settings_outcome(r, &reply->loading, &reply->changed, &reply->found);
    if (outcome != 0 || !reply->found)
        return outcome;
    uint32_t count = (uint32_t)r < SETTINGS_PAGE ? (uint32_t)r : SETTINGS_PAGE;
    for (uint32_t i = 0; i < count; i++) {
        const dk_project_setting *s = &settings[i];
        project_manager_Setting *out = &reply->entries[i];
        out->key_len = project_query_text(out->key, sizeof out->key, s->key,
                                          s->key_len < sizeof s->key ? s->key_len : sizeof s->key);
        out->value_len = project_query_value(out->value, sizeof out->value, s->head, s->head_len);
        out->value_length = s->value_len;
    }
    reply->entries_count = count;
    reply->next = next;
    return 0;
}

int32_t project_manager_read_file(dk_module_ctx *ctx, const project_manager_ReadFileRequest *request,
                                  project_manager_ReadFileReply *reply)
{
    (void)ctx;
    uint32_t total = 0;
    int32_t r = dk_project_read(request->slot, request->version, request->offset, reply->data,
                                READ_PAGE, &total);
    int32_t outcome =
        project_query_settings_outcome(r, &reply->loading, &reply->changed, &reply->found);
    if (outcome != 0 || !reply->found)
        return outcome;
    reply->data_len = (uint32_t)r < READ_PAGE ? (uint32_t)r : READ_PAGE;
    reply->total = total;
    return 0;
}
