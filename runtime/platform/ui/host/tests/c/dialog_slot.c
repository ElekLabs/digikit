/* Host test of the open dialog's slot of core/ui (src/dialog_slot.h, the same source ui.c links). */
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include "../../../src/dialog_slot.h"

static ui_dialog_slot slot;
static int calls[4];
static int32_t last_answer[4];

static void done(void *arg, int32_t answer)
{
    int tag = (int)(intptr_t)arg;
    calls[tag]++;
    last_answer[tag] = answer;
}

void ds_reset(void)
{
    memset(&slot, 0, sizeof slot);
    memset(calls, 0, sizeof calls);
    memset(last_answer, 0, sizeof last_answer);
}
int ds_begin(uint32_t serial, uintptr_t owner, int tag)
{
    return ui_dialog_begin(&slot, serial, owner, done, (void *)(intptr_t)tag);
}
int ds_begin_no_done(uint32_t serial, uintptr_t owner)
{
    return ui_dialog_begin(&slot, serial, owner, 0, 0);
}
int ds_is_open(void)
{
    return ui_dialog_is_open(&slot);
}
void ds_detach(uint32_t serial)
{
    ui_dialog_detach(&slot, serial);
}
int ds_answer(int32_t answer)
{
    return ui_dialog_answer(&slot, answer);
}
int ds_drop_owner(uintptr_t owner)
{
    return ui_dialog_drop_owner(&slot, owner);
}
int ds_calls(int tag)
{
    return calls[tag];
}
int32_t ds_last_answer(int tag)
{
    return last_answer[tag];
}
