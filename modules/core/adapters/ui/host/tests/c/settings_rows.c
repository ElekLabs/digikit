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
int sr_add(uintptr_t owner, const char *name, const char *text, int tag)
{
    return ui_rows_add(&reg, owner, name, 1, text, open_a, (void *)(intptr_t)tag);
}
int sr_add_outside_init(uintptr_t owner, const char *text)
{
    return ui_rows_add(&reg, owner, "m", 0, text, open_a, 0);
}
int sr_add_no_open(uintptr_t owner, const char *text)
{
    return ui_rows_add(&reg, owner, "m", 1, text, 0, 0);
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
uintptr_t sr_key(uint32_t i)
{
    return (uintptr_t)reg.row[i].key;
}
int sr_run(uintptr_t key)
{
    return ui_rows_run(&reg, (const ui_row_key *)key);
}
int sr_calls(int tag)
{
    return calls[tag];
}
