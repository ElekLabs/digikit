| core/dyndsp on the Digitakt II: the table of DSP addresses the digikit runtime reads.
| The numbers (device id, area size, rows) are the device file's, through build/gen/device-dt2.inc (`patchkit gen`).
| Design: docs/design/digikit-dkm.md ("DSP section"). All words big endian; the DSP symbols are in their natural units
| (a byte address for data, a short-word pc for code).
|
|  0 'DDSP'   4 version   8 device ([digikit] id: 1 = dt2)
| 12 area base (the DSP byte address of core/dsp-link's reserved area = window 0)
| 16 area bytes (DL_AREA_BYTES of modules/core/adapters/dsp-link/src/dsp/dt2.s)
| 20 voice rows base (the voices registry = window 1)   24 index of the first free row   28 number of free rows
| 32 number of rows in the registry (window 1 is 2 words per row)
| 36 import count N; then N entries of 32 bytes: a NUL padded name of 28 bytes and its value

    .include "device-dt2.inc"

    .section .rodata
    .balign 4
    .globl dyndsp_info
dyndsp_info:
    .long   0x44445350
    .long   1
    .long   DEVICE_ID
    .long   _dl_area
    .long   DL_AREA_BYTES
    .long   _dd_vrows
    .long   _dd_vfirst
    .long   DYN_TYPES
    .long   _dd_vcount
    .long   4
    .asciz  "_dv_exp2"
    .space  28 - 9
    .long   _dv_exp2
    .asciz  "_dv_frame_hw"
    .space  28 - 13
    .long   _dv_frame_hw
    .asciz  "_dv_frame_sw"
    .space  28 - 13
    .long   _dv_frame_sw
    .asciz  "_dk_step"
    .space  28 - 9
    .long   _dk_step
