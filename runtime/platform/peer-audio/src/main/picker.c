#include <dk/gen/imports.h>
#include "../wire.h"
#include "dk_fw_menu.h"
#include "dk_fw_machine.h"
#include "dk_ui_core.h"

extern int32_t dk_peer_audio_select_track(uint32_t track);
extern uint32_t dk_peer_audio_requested_track(void);
extern int32_t dk_peer_audio_status(void);
extern void peer_picker_label(void);

/* The display treats -1 as a divider and 0x2d and above as having no stock group.
 * This ID is private to MenuItem. It never enters the stock machine catalog or kit. */
#define PEER_ROW 0x2du
/* The peer mixer needs a stock audio lane. A track configured as MIDI has none. */
#define STOCK_MIDI 0x23u

static char machine_label[16];
static MenuItem machine_row;

/* UI task only. Copy the label so the view never borrows module memory. */
void dk_peer_picker_offer(const char *label)
{
    unsigned i = 0;
    while (i + 1 < sizeof machine_label && label[i]) {
        machine_label[i] = label[i];
        ++i;
    }
    machine_label[i] = 0;
}

/* The label reads resident data and has no capture to copy or destroy. */
static uint32_t label_manager(void)
{
    return 0;
}

void peer_picker_copy_label(uint32_t *out)
{
    uint8_t allocator = 0;
    dk_fw_string_std_string_ctor(out, machine_label, &allocator);
}

static MenuItem *item(const shared_ptr *entry)
{
    return (MenuItem *)(uintptr_t)entry->ptr;
}

static void highlight(PeerMachineList *view, uint32_t row_id)
{
    dk_fw_machine_peer_picker_menu_select(&view->menu, row_id, 0);
    dk_fw_view_invalidate((uint8_t *)view);
}

void peer_picker_install(uint8_t *raw_view, uint32_t stock_type)
{
    PeerMachineList *view = (PeerMachineList *)raw_view;
    dk_fw_machine_peer_picker_stock_select(raw_view, stock_type);
    if (view->track >= PEER_AUDIO_TRACKS || stock_type == STOCK_MIDI || !machine_label[0])
        return;

    /* Borrow a resident MenuItem in spare vector capacity. A null control block
     * means stock can copy and discard the entry without owning or freeing it. */
    if (view->items_end >= view->items_capacity)
        return;
    if (machine_row.id != PEER_ROW) {
        static const std_function label = {.manager = (uintptr_t)label_manager,
                                           .invoker = (uintptr_t)peer_picker_label};
        static const std_function empty;
        dk_fw_menu_item_ctor((uint8_t *)&machine_row, &label, &empty, &empty, &empty, PEER_ROW, 8);
    }
    shared_ptr *row = (shared_ptr *)(uintptr_t)view->items_end;
    *row = (shared_ptr){.ptr = (uintptr_t)&machine_row};
    view->items_end += sizeof *row;
    if (dk_peer_audio_requested_track() == view->track + 1)
        highlight(view, PEER_ROW);
}

static const shared_ptr *peer_row(const PeerMachineList *view, const shared_ptr *begin)
{
    const shared_ptr *end = (const shared_ptr *)(uintptr_t)view->items_end;
    if (begin == end || item(end - 1)->id != PEER_ROW)
        return NULL;
    return end - 1;
}

/* Use the stock preview delta while crossing stock rows. Dividers are menu rows
 * only. When the cursor is on the peer row, stock_type remains the preview base. */
int32_t peer_picker_move(uint8_t *callback, int32_t delta)
{
    PeerMachineList *view =
        (PeerMachineList *)(callback - offsetof(PeerMachineList, move_callback));
    const shared_ptr *begin = (const shared_ptr *)(uintptr_t)view->items_begin;
    const shared_ptr *last = peer_row(view, begin);
    if (!last || view->selected > (size_t)(last - begin) || delta == 0)
        return delta;

    const shared_ptr *cursor = begin + view->selected;
    bool on_peer = cursor == last;
    if (on_peer && delta > 0)
        return 0;
    if (!on_peer && delta < 0)
        return delta;

    if (on_peer) {
        for (cursor = begin; cursor < last && item(cursor)->id != view->stock_type; ++cursor) {
        }
        if (cursor == last)
            return delta;
    }
    int32_t steps = 0;
    for (const shared_ptr *row = cursor + 1; row < last; ++row) {
        if (item(row)->id != UINT32_MAX)
            ++steps;
    }
    if (on_peer) {
        int32_t stock_delta = delta + steps + 1;
        if (stock_delta == 0)
            highlight(view, view->stock_type);
        return stock_delta;
    }
    if (delta > steps) {
        highlight(view, PEER_ROW);
        return 0;
    }
    return delta;
}

/* Returns 0 to confirm stock, 1 to use stock cancel, and 2 to keep the view open. */
uint32_t peer_picker_confirm(PeerMachineList *view)
{
    const shared_ptr *begin = (const shared_ptr *)(uintptr_t)view->items_begin;
    const shared_ptr *last = peer_row(view, begin);
    if (!last)
        return 0;
    uint32_t track = view->track + 1;
    if (view->selected == (size_t)(last - begin)) {
        if (dk_peer_audio_status() <= 0) {
            dk_ui_toast(dk_ui_core_handle(), "SAMPLE UNAVAILABLE");
            return 2;
        }
        if (dk_peer_audio_select_track(track) == 0)
            return 1;
    } else {
        if (dk_peer_audio_requested_track() != track || dk_peer_audio_select_track(0) == 0)
            return 0;
    }
    dk_ui_toast(dk_ui_core_handle(), "SAMPLE BUSY");
    return 2;
}
