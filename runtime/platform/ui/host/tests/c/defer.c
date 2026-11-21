/* Host test of the key-release queue of core/ui (src/key_defer.h, the same source ui.c links). */
#include <stdint.h>
#include <string.h>
#include "../../../src/key_defer.h"

static ui_defer d;
static int starts, start_fail, down_state;
static int ran[8];
static uint32_t order[8];
static int order_n;

static int32_t start(void *ctx)
{
    (void)ctx;
    starts++;
    return start_fail ? -1 : 0;
}
static void act(void *ctx, uint32_t arg)
{
    (void)ctx;
    ran[arg]++;
    order[order_n++ % 8] = arg;
}
static int is_down(void *k, uint32_t code)
{
    (void)k;
    (void)code;
    return down_state;
}

void df_reset(void)
{
    memset(&d, 0, sizeof d);
    memset(ran, 0, sizeof ran);
    order_n = starts = start_fail = down_state = 0;
    d.start = start;
}
int32_t df_add_in_key(uintptr_t owner, uint32_t arg, uint32_t code)
{
    int32_t r;
    ui_defer_enter(&d, code);
    r = ui_defer_add(&d, owner, act, 0, arg);
    ui_defer_leave(&d);
    return r;
}
int df_active_in_key(void)
{
    int a;
    ui_defer_enter(&d, 1);
    a = ui_defer_active(&d);
    ui_defer_leave(&d);
    return a && !ui_defer_active(&d);
}
int df_released(uint32_t code)
{
    return ui_defer_released(&d, code);
}
void df_set_down(int on)
{
    down_state = on;
}
void df_set_start_fail(int on)
{
    start_fail = on;
}
void df_poll(void)
{
    ui_defer_poll(&d, is_down, 0);
}
uint32_t df_drop(uintptr_t owner)
{
    return ui_defer_drop_owner(&d, owner);
}
int df_cancel(uint32_t arg)
{
    return ui_defer_cancel(&d, act, 0, arg);
}
uint32_t df_pending(void)
{
    return ui_defer_pending(&d);
}
int df_ran(uint32_t arg)
{
    return ran[arg];
}
uint32_t df_order(int i)
{
    return order[i];
}
int df_starts(void)
{
    return starts;
}
