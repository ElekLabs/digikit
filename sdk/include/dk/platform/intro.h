/* <dk/platform/intro.h>: a module's own boot animation (the import dk_intro_take). `dk_intro_take(frame, done, arg, ticks)` in `init` replaces the stock
   boot animation. The firmware's intro runs at 30 ticks a second; the module gives one 128 x 64 native bitmap per tick. It runs after the boot checks
   (REC, safe mode, boot counter) and uses no storage: read the frames in `init`.

     frame:   `const uint8_t *frame(uint32_t tick, void *arg)` returns the 1024-byte native bitmap of this tick (0 .. ticks - 1), or NULL to end the
              animation (the last frame stays on the screen). It runs on the intro task, not on the UI task: it must not block and must not call
              any import. The bitmap must stay valid until the next call of frame.
     done:    `void done(void *arg)`, may be NULL. Called once on the UI task at the first UI pass, after the intro has ended. Free the
              buffers here: never allocate or free inside frame.
     ticks:   1 to 300.

   Only inside `init`; the last call of `init` is the best place, so that a later failure does not leave the intro with a module that
   was rolled back. Results: 0, E_NOT_FOUND (outside `init`), E_CONTRACT (no frame, or ticks out of range), E_BUSY (another module took the
   intro) or E_BOOT_ONLY (the intro is over already). A refusal changes nothing: the stock intro runs.

   A fault inside frame reaches the firmware's own exception handler. The boot counter stays raised until the session is stable, so repeated
   failed boots end in safe mode, where no module loads. */
#ifndef DK_INTRO_H
#define DK_INTRO_H
#include <dk/core/module.h>
#include <dk/core/status.h>

#define DK_INTRO_TICKS_MAX 300u
#define DK_INTRO_BYTES 1024u /* the native bitmap of one tick */

/* The runtime's results (sdk/sdk.toml [load.status]) named above. */
#define DK_INTRO_E_NOT_FOUND DK_E_NOT_FOUND /* E_NOT_FOUND: outside init */
#define DK_INTRO_E_BUSY DK_E_BUSY           /* E_BUSY: taken already */
#define DK_INTRO_E_BOOT_ONLY DK_E_BOOT_ONLY /* E_BOOT_ONLY: the intro is over */
#define DK_INTRO_E_CONTRACT DK_E_CONTRACT   /* E_CONTRACT: no frame, or ticks out of range */

#endif
