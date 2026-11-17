| core/dyndsp on the Digitone II: the table of DSP addresses the digikit runtime reads.
| The numbers (device id, area size, rows) are the device file's, through build/gen/device-dn2.inc (`patchkit gen`).
| Design: docs/design/digikit-dkm.md ("DSP section"). All words big endian; the DSP symbols are in their natural units
| (a byte address for data, a short-word pc for code).
|
|  0 'DDSP'   4 version   8 device ([digikit] id: 2 = dn2)
| 12 area base (the DSP byte address of core/dsp-link's reserved area = window 0)
| 16 area bytes (DL_AREA_BYTES - DL_SLOT_BYTES: window 0 starts with the slot block `_dl_slots`, which is no part of the area; dn2.s of core/dsp-link)
| 20 voice rows base (the voices registry = window 1)   24 index of the first free row   28 number of free rows
| 32 number of rows in the registry (window 1 is 2 words per row)
| 36 import count N; then N entries of 32 bytes: a NUL padded name of 28 bytes and its value

    .include "device-dn2.inc"

| One import: the name in a 28 byte NUL padded field, then the value (a DSP symbol, or a firmware anchor that the object's externs name).
    .macro DSP_IMPORT sym
    .Lstart\@ = .
    .asciz  "\sym"
    .space  28 - (. - .Lstart\@)
    .long   \sym
    .endm

| DK_SLOT_BYTES of sdk/dsp/contract/dk_stub.inc: the slot block at the start of window 0.
    .equ    DL_SLOT_BYTES, 64

    .section .rodata
    .balign 4
    .globl dyndsp_info
dyndsp_info:
    .long   0x44445350
    .long   1
    .long   DEVICE_ID
    .long   _dl_area
    .long   DL_AREA_BYTES - DL_SLOT_BYTES
    .long   _dd_vrows
    .long   _dd_vfirst
    .long   DYN_TYPES
    .long   _dd_vcount
    .long   5
    DSP_IMPORT _dv_exp2
    DSP_IMPORT _dv_frame_hw
    DSP_IMPORT _dv_frame_sw
    DSP_IMPORT _dk_step
    DSP_IMPORT _dl_slots
