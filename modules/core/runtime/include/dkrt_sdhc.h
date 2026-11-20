/* The large card command plug-in point of core/sdhc-large. NOT an SDK contract: content modules use `dk_sdhc_large_*` of <dk/platform/sdhc_large.h>.
   The runtime (store_writer.rs) calls the optional symbols sdhc_large_range and sdhc_large_ctl that modules/core/adapters/sdhc-large exports.
   Without the adapter the symbols resolve to 0: the range call is skipped and the switch reads as off. docs/findings/126-usb-fast-transfer-ceilings.md. */
#ifndef DKRT_SDHC_H
#define DKRT_SDHC_H
#include <stdint.h>

/* The firmware's eMMC block write and read run a transfer as one card command of up to 1022 blocks (511 KiB) instead of 64 KiB pieces when its
   buffer lies inside [lo, hi), is 16-byte aligned and its length is a multiple of 512, while the switch is on (sdhc_large_ctl key 0). `lo` = `hi` = 0
   clears the range. One range at a time: the caller sets it before the transfer and clears it after. */
void sdhc_large_range(uint32_t lo, uint32_t hi);

/* Key 0 is the switch (val 0 off, 1 on, another value changes nothing; returns the old value; RAM only, 1 at every boot). Key 1 reads a counter
   (val 0 calls of the firmware's block write, 1 of them with the raised limit, 2 calls of the block read, 3 of them with the raised limit).
   Other keys return 0. */
uint32_t sdhc_large_ctl(uint32_t key, uint32_t val);
#endif
