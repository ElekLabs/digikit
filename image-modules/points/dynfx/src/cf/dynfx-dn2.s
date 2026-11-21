| core/dynfx on the Digitone II: the table of the effect chain that the digikit runtime reads.
| The numbers (device id, free rows) are the device file's, through build/gen/device-dn2.inc (`patchkit gen`).
| Design: docs/design/digikit-dkm.md ("DSP section", effect records). All words big endian; the row base is a DSP byte address.
|
|  0 'DFXI'   4 version   8 device ([digikit] id: 2 = dn2)
| 12 rows base (the registry `fx` of core/fx = window 2 of core/dsp-link)   16 index of the first free row   20 number of free rows
| 24 number of rows in the registry (free rows, and the rows of patched-in effects)   28 words of a row (24)

    .include "device-dn2.inc"

    .section .rodata
    .balign 4
    .globl dynfx_info
dynfx_info:
    .long   0x44465849
    .long   1
    .long   DEVICE_ID
    .long   _df_rows
    .long   _df_first
    .long   LIMIT_FX_SLOTS
    .long   _df_count
    .long   24
