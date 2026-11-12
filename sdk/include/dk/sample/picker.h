/* <dk/sample/picker.h>: the on-device sample picker of a sampler machine that owns a 16-entry sample pool (<dk/sample/pool.h>): the
   chooser (<dk/chooser/chooser.h>) filled with the pool. Shared source, not a runtime call: the packer links sdk/src/sample/picker.c and the chooser and core
   sources it needs (design: docs/design/sample-picker.md, docs/design/ui-rules.md).

   The module calls three functions:

     init:   dk_sample_pool_open(ctx, &pool); dk_files_open(ctx, &files); dk_ui_open(ctx, &ui);
             dk_sample_picker_init(pool, files, ui);
     click:  dk_sample_picker_open_pool(cur, pick, arg);   the SMP knob was clicked (released without a turn), see below
     fini:   dk_sample_picker_fini();

   dk_sample_picker_open_pool opens the slot list (the generic slot list of <dk/chooser/chooser.h>) over the machine page: the 16 slots, numbered 0 to
   15 like the SMP knob, with name, mono/stereo, length or state; an empty slot shows "-". The slot the SMP value points at is marked with
   '>' and the cursor starts on it.
     YES on a slot    opens the folder browser of the samples source (<dk/platform/files.h>) for that slot (empty or filled)
     YES on a file    loads it into that slot (dk_sample_pool_set; a filled slot is replaced), calls pick(arg, slot) and closes the whole
                      flow: one press, back on the page
     RIGHT on a slot  CLEAR (dk_sample_pool_clear) and INFO; RIGHT on a file in the browser gives SELECT and INFO
     NO               up one folder in the browser, out of the browser at the root back to the slot list; in the list back to the page
   A load that the runtime refuses (bad path, no pool on this device) closes the flow and shows the error text as a toast on the
   page; pick is not called. pick is how the machine sets its SMP value: the picker has no way to write a parameter itself. It runs in the
   UI task, after the lists have closed. The picker does nothing when it could not start, or when
   init was not called. The three handles are the module's own: the picker takes no module context. */
#ifndef DK_SAMPLE_PICKER_H
#define DK_SAMPLE_PICKER_H
#include <stdint.h>
#include <dk/core/module.h>
#include <dk/platform/files.h>
#include <dk/platform/ui.h>

/* Called with the slot the machine should select: the slot a file was just loaded into. arg stays in use after the
   click callback returns: static or module memory. */
typedef void dk_sample_picker_pick_fn(void *arg, int32_t index);

/* Call in `init` with the module's handles: its sample pool (dk_sample_pool_open), the listing client (dk_files_open) and the UI (dk_ui_open). The picker
   keeps them. DK_OK when the picker can run, DK_E_MISSING for a NULL handle (the click then does nothing). */
int32_t dk_sample_picker_init(dk_sample_pool *pool, dk_files *files, dk_ui *ui);
/* Opens the slot list. current is the SMP value now (0 to 15, or -1) and is marked (-1: no row is marked); pick may be 0: loading a file then only loads it. Call from the UI task. */
void dk_sample_picker_open_pool(int32_t current, dk_sample_picker_pick_fn *pick, void *arg);
/* Stops the picker's timer and listing; call in `fini`. */
void dk_sample_picker_fini(void);
#endif
