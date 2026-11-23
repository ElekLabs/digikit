/* The one open dialog of core/ui (dk_ui_confirm, dk_ui_choice): a small slot with no firmware and no runtime call, so the host can test it
   (runtime/platform/ui/host/tests/dialog_slot.rs). ui.c owns the one slot. The slot holds the serial of the open window, who asked for it (the owner
   handle of dk_ui_open) and the answer function with its argument. The window keeps no pointer into the owner's code: it only knows the serial. So when the
   owner is unloaded while its dialog is open, `ui_dialog_drop_owner` takes the answer function out of the slot: the window stays until it closes (the
   firmware owns it), its answer calls nothing, and the code of the unloaded module is never called. */
#ifndef DK_UI_DIALOG_SLOT_H
#define DK_UI_DIALOG_SLOT_H
#include <stdint.h>

typedef void ui_dialog_done_fn(void *arg, int32_t answer);

typedef struct ui_dialog_slot {
    uint32_t serial; /* serial of the open window, 0: none */
    uintptr_t owner;
    ui_dialog_done_fn *done; /* 0: dropped with its owner (or none open) */
    void *arg;
} ui_dialog_slot;

/* Is a dialog open? (A window whose owner is gone still counts: it is on the screen until it closes.) */
int ui_dialog_is_open(const ui_dialog_slot *s);

/* Open a dialog: the slot takes `serial` (not 0), the owner and the answer function. 0, or -1 when a dialog is open. */
int32_t ui_dialog_begin(ui_dialog_slot *s, uint32_t serial, uintptr_t owner,
                        ui_dialog_done_fn *done, void *arg);

/* The window `serial` left the screen (any way of closing): the slot is free. No effect for another serial. */
void ui_dialog_detach(ui_dialog_slot *s, uint32_t serial);

/* The window answered: the slot is free first, then the owner's function runs with `answer`, if it is still there. 1 when it ran, 0 when there was none
   (the owner was unloaded) or no dialog was open. */
int ui_dialog_answer(ui_dialog_slot *s, int32_t answer);

/* `owner` is unloaded: its open dialog (if any) loses its answer function. 1 when a function was dropped. */
int ui_dialog_drop_owner(ui_dialog_slot *s, uintptr_t owner);

#endif
