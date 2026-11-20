/* Host test of the SETTINGS rows registry of core/ui (src/settings_rows.h, the same source ui.c links). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/settings_rows.h"

static ui_settings_rows reg;
static int calls[8];
static void *last_arg[8];

static void open_a(void *arg)
{
    int tag = (int)(intptr_t)arg;
    calls[tag]++;
    last_arg[tag] = arg;
}

void sr_reset(void)
{
    memset(&reg, 0, sizeof reg);
    memset(calls, 0, sizeof calls);
}
int sr_add(uintptr_t owner, const char *text, int tag)
{
    return ui_rows_add(&reg, owner, text, open_a, (void *)(intptr_t)tag);
}
int sr_add_no_open(uintptr_t owner, const char *text)
{
    return ui_rows_add(&reg, owner, text, 0, 0);
}
uint32_t sr_remove(uintptr_t owner)
{
    return ui_rows_remove_owner(&reg, owner);
}
uint32_t sr_count(void)
{
    return reg.n;
}
const char *sr_text(uint32_t i)
{
    return reg.row[i].text;
}
uint32_t sr_id(uint32_t i)
{
    return reg.row[i].id;
}
int sr_run(uint32_t id)
{
    return ui_rows_run(&reg, id);
}
int sr_calls(int tag)
{
    return calls[tag];
}
