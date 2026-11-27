/* Isolate the ordinary machine mapping tests from the optional callback-page provider: the entries of the machines service (dks/src/native.rs) the hooks call, with no pages. */
    .text
    .globl dks_native_page_count
dks_native_page_count:
    move.l 8(%sp),%d0
    rts
    .globl dks_native_descriptor
dks_native_descriptor:
    moveq #0,%d0
    rts
    .globl dks_native_code
dks_native_code:
    moveq #-1,%d0
    rts
    .globl dks_native_draw
dks_native_draw:
    moveq #0,%d0
    rts
    .globl dks_native_overlay
dks_native_overlay:
    rts
