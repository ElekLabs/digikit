/* core/card-guard: the two hook shims. Each is entered by a jump from the first eight bytes of a firmware function (a `before` hook: the displaced
 * `lea (-N,SP),SP; movem.l ...,(SP)` are copied to the tail). At the entry the stack holds the caller's frame: 0(SP) = return address, 4(SP) = the first
 * argument, 8(SP) = the second. D0, D1, A0, A1 and the condition codes are dead there (finding 156).
 *
 * The shim pushes (return address, second, first) and calls dkcg_write / dkcg_erase of guard.c, `int32_t f(first, second, ret)`. 0: the displaced code runs
 * and the stock function continues. Anything else: the request is refused; the shim returns to the caller with that value in D0, before the stock code
 * has taken the card lock or changed anything. A `move.l d(SP),-(SP)` reads its source with the SP before the push, so 12(SP) is the right offset for each
 * of the three pushes (the frame is a word deeper after each). */
    .text
    .even

/* i32 (u32 sector, u32 bytes, const u8 *buf) */
    .globl dkcg_write_hook
dkcg_write_hook:
    move.l  (%sp),-(%sp)            /* ret */
    move.l  12(%sp),-(%sp)          /* bytes */
    move.l  12(%sp),-(%sp)          /* sector */
    jsr     dkcg_write
    lea     12(%sp),%sp
    tst.l   %d0
    bne.b   1f
    jmp     cg_write_next
1:  rts

/* i32 (u32 sector, u32 count) */
    .globl dkcg_erase_hook
dkcg_erase_hook:
    move.l  (%sp),-(%sp)            /* ret */
    move.l  12(%sp),-(%sp)          /* count */
    move.l  12(%sp),-(%sp)          /* sector */
    jsr     dkcg_erase
    lea     12(%sp),%sp
    tst.l   %d0
    bne.b   1f
    jmp     cg_erase_next
1:  rts
