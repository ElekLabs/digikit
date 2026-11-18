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
    .long   61
    DSP_IMPORT _dv_exp2
    DSP_IMPORT _dv_frame_hw
    DSP_IMPORT _dv_frame_sw
    DSP_IMPORT _dk_step
    DSP_IMPORT _dl_slots
    DSP_IMPORT _dv_sv_m13
    DSP_IMPORT _dv_sv_i12
    DSP_IMPORT _dv_idle
    DSP_IMPORT _fw_frame
    DSP_IMPORT _dv_table
    DSP_IMPORT _gs_rbuf
    DSP_IMPORT _gs_post
    DSP_IMPORT _dv_save
    DSP_IMPORT _dv_s_i4
    DSP_IMPORT _dv_s_l4
    DSP_IMPORT _dv_mask
    DSP_IMPORT _dv_q1
    DSP_IMPORT _dv_q2
    DSP_IMPORT _dv_q4
    DSP_IMPORT _dv_ql4
    DSP_IMPORT _dv_ctx
    DSP_IMPORT _dv_diag
    DSP_IMPORT _dv_rec
    DSP_IMPORT _dv_t
    DSP_IMPORT _dv_typ
    DSP_IMPORT _dv_buf
    DSP_IMPORT _dv_row
    DSP_IMPORT _dv_in
    DSP_IMPORT _dv_params
    DSP_IMPORT _dv_call_params
    DSP_IMPORT _dv_trig_params
    DSP_IMPORT _dv_c
    DSP_IMPORT _dv_cst
    DSP_IMPORT _dv_slots
    DSP_IMPORT _fx_table
    DSP_IMPORT _fx_master
    DSP_IMPORT _fx_sv_m13
    DSP_IMPORT _fx_sv_i12
    DSP_IMPORT _fx_idle
    DSP_IMPORT _fx_s_i4
    DSP_IMPORT _fx_s_l4
    DSP_IMPORT _fx_save
    DSP_IMPORT _fx_ctx
    DSP_IMPORT _fx_blk
    DSP_IMPORT _fx_in
    DSP_IMPORT _fx_scratch
    DSP_IMPORT _fx_c
    DSP_IMPORT _fx_diag
    DSP_IMPORT _fx_rec
    DSP_IMPORT _fxs_idle
    DSP_IMPORT _fxs_return
    DSP_IMPORT _fxs_bus
    DSP_IMPORT _fxs_gnew
    DSP_IMPORT _fxs_prev
    DSP_IMPORT _fxs_c
    DSP_IMPORT _fxs_slotbus
    DSP_IMPORT _fxs_slotmap
    DSP_IMPORT _fxs_fadetab
    DSP_IMPORT _fxs_trk
    DSP_IMPORT _fxs_flg
    DSP_IMPORT _fxs_fbuf
