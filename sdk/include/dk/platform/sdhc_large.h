/* <dk/platform/sdhc_large.h>: large card commands (needs core/sdhc-large). Hand-written.

   `dk_sdhc_large_open(ctx, &sl)` in `init`; without core/sdhc-large it returns DK_E_MISSING and `sl` stays NULL. `dk_sdhc_large_ctl(sl, key, val)`
   then still works: the stub returns 0, so a module can go on without the adapter and treat every result as "off, nothing to set".
   With the adapter, the firmware's eMMC block write and read run a transfer as one card command of up to 1022 blocks (511 KiB) instead of 64 KiB
   pieces when the buffer lies inside the range the runtime registers for its own card transfers, is 16-byte aligned and its length is a multiple of
   512, while the switch is on. Key 0 is the switch (val 0 off, 1 on, another value changes nothing; returns the old value; RAM only, 1 at every boot).
   Key 1 reads a counter (val 0 calls of the firmware's block write, 1 of them with the raised limit, 2 calls of the block read, 3 of them with the
   raised limit). Other keys return 0. */
#ifndef DK_SDHC_LARGE_H
#define DK_SDHC_LARGE_H
#include <dk/core/module.h>
#endif
