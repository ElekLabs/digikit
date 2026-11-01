/* <dk/sample_picker.h>: the on-device sample picker for a module that owns a 16-track sample table (<dk/sample_track.h>).
   Shared source, not a runtime call: the module lists sdk/src/sample_picker.c and sdk/src/picker_logic.c in its [[object]] sources and
   needs no new manifest field (design: docs/design/sample-picker.md).

   The module calls two functions:

     dkm_init:   dk_picker_init(api, handle);   handle is the value of api->sample_tracks(local_id)
     dkm_menu:   dk_picker_open(m);             in the fill of its settings menu (DK_MENU)
     dkm_fini:   dk_picker_fini();

   dk_picker_open adds one row, SAMPLES..., to the settings menu m. It adds nothing when the device has no core/ui lists, when the
   picker could not start, or when init was not called. The row opens the track list: 16 rows with the state of each track. YES on a
   track opens the folder browser of the samples source (<dk/files.h>); YES on a file selects it for that track. RIGHT on a track gives
   CLEAR and INFO, RIGHT on a file gives SELECT and INFO. NO goes up one folder, and out of the browser at the root. A select that the
   runtime refuses at once is shown in the track row; the old sample stays bound. */
#ifndef DK_SAMPLE_PICKER_H
#define DK_SAMPLE_PICKER_H
#include <dk/api.h>
#include <dk/ui.h>

/* Acquires the listing client; call in dkm_init. The picker keeps api; it must stay valid. 0 when the picker can run, -1 when not (the
   module still works, the picker row is absent). */
int dk_picker_init(const dk_api *api, uint32_t handle);
/* Adds the SAMPLES... row to the menu m. Call from the fill of the settings menu. */
void dk_picker_open(dk_ui_menu *m);
/* Stops the picker's timer and listing; call in dkm_fini. */
void dk_picker_fini(void);
#endif
