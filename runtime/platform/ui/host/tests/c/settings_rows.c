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

/* A stand-in for the runtime's dkrt_record_name: record 2 is "svc.two", record 3 has 31 characters, record 4 fills all 32 bytes, every other record is not loaded. */
static int32_t fake_record_name(uint32_t record, uint8_t name[DK_UI_MODULE_NAME])
{
    memset(name, 0, DK_UI_MODULE_NAME);
    if (record == 2)
        memcpy(name, "svc.two", 8);
    else if (record == 3)
        memset(name, 'a', 31);
    else if (record == 4)
        memset(name, 'b', 32);
    else
        return -1;
    return 0;
}
int sr_owner_name(uintptr_t handle, char *out)
{
    return ui_owner_name(handle, fake_record_name, out);
}
