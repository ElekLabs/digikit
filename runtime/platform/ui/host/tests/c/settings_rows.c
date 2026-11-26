/* Host test of the SETTINGS rows registry of core/ui (src/settings_rows.h, the same source ui.c links). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/settings_rows.h"

static ui_settings_rows reg;
static int rebuilds;
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
    rebuilds = 0;
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

/* The stock list rebuild trigger. The fake rebuild does what the stock one does through the hook: it ends in ui_rows_built. */
#define FAKE_VTABLE 0x40203bd8u
static uint32_t fake_menu[2] = {FAKE_VTABLE, 0}; /* first word: the vtable of a SETTINGS menu */
static void fake_rebuild(uint8_t *menu)
{
    rebuilds++;
    ui_rows_built(&reg, menu);
}
void sr_built(int with_menu)
{
    ui_rows_built(&reg, with_menu ? (uint8_t *)fake_menu : 0);
    fake_menu[0] = FAKE_VTABLE;
    rebuilds = 0;
}
int sr_pass(void)
{
    return ui_rows_pass(&reg, fake_rebuild, FAKE_VTABLE);
}
void sr_menu_word(uint32_t w)
{
    fake_menu[0] = w;
}
/* the anchors of the rebuild or of the vtable are absent on this firmware: 0 */
int sr_pass_absent(int no_rebuild)
{
    return ui_rows_pass(&reg, no_rebuild ? 0 : fake_rebuild, no_rebuild ? FAKE_VTABLE : 0);
}
int sr_has_menu(void)
{
    return reg.menu != 0;
}
int sr_rebuilds(void)
{
    return rebuilds;
}
int sr_dirty(void)
{
    return ui_rows_dirty(&reg);
}
