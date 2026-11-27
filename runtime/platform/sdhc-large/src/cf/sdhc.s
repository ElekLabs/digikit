/* core/sdhc-large (DT2 1.17 and DN2 1.12, the same code at the same offsets on both). Hooks in the firmware's eSDHC block write and read: the four
 * `cmpi.l #0x10000,D2` of the 64 KiB split (the entry test and the chunk loop of each function) compare with a limit worked out here instead.
 * The limit is 1022 blocks (0x7fc00: the eDMA's CITER holds 15 bits, 32 per block, so 1023 is the most; 1022 keeps one block of margin) when ALL hold:
 *  - the switch is on (sl_off = 0, the boot value),
 *  - D3 (the buffer of the command; the chunk loop advances it) is 16-byte aligned and D2 (the bytes) is a multiple of 512: the firmware's bounce path
 *    (a 64 KiB buffer) is used for other buffers, and it holds 64 KiB at most,
 *  - [D3, D3 + D2) lies inside the registered range [sl_lo, sl_hi) (sdhc_large_range in sdhc.c).
 * Otherwise the limit is the stock 0x10000 and the firmware behaves byte for byte as before. D0 and D1 are scratch at these sites. */
    .text
    .even

/* in D2 bytes, D3 buffer; out D0 limit; clobbers D1 */
sl_limit:
    move.l  #0x10000,%d0
    tst.b   sl_off
    bne.b   9f
    move.l  %d3,%d1
    andi.l  #15,%d1
    bne.b   9f
    move.l  %d2,%d1
    andi.l  #511,%d1
    bne.b   9f
    cmp.l   sl_lo,%d3                  /* D3 below the range */
    bcs.b   9f
    move.l  %d3,%d1
    add.l   %d2,%d1                    /* the end of the transfer; a wrap is no transfer of ours */
    bcs.b   9f
    cmp.l   sl_hi,%d1                  /* the end past the range (also: no range, hi = 0) */
    bhi.b   9f
    move.l  #0x7fc00,%d0
9:  rts

/* `cmpi.l #0x10000,D2` of the entry test, then `ble`: count the call (one call = one card command) and compare with the limit. */
    .globl sdhc_large_entry_w
sdhc_large_entry_w:
    bsr.w   sl_limit
    addq.l  #1,sl_cnt
    cmpi.l  #0x10000,%d0
    beq.b   1f
    cmpi.l  #0x10000,%d2
    ble.b   1f
    addq.l  #1,sl_cnt+4
1:  cmp.l   %d0,%d2
    jmp     sl_wr_entry+6

/* `cmpi.l #0x10000,D2; ble; moveq 1,D5; swap D5` of the chunk loop: D5 = min(D2, limit). */
    .globl sdhc_large_chunk_w
sdhc_large_chunk_w:
    bsr.w   sl_limit
    cmp.l   %d0,%d2
    ble.b   1f
    move.l  %d0,%d5
1:  jmp     sl_wr_chunk+12

    .globl sdhc_large_entry_r
sdhc_large_entry_r:
    bsr.w   sl_limit
    addq.l  #1,sl_cnt+8
    cmpi.l  #0x10000,%d0
    beq.b   1f
    cmpi.l  #0x10000,%d2
    ble.b   1f
    addq.l  #1,sl_cnt+12
1:  cmp.l   %d0,%d2
    jmp     sl_rd_entry+6

    .globl sdhc_large_chunk_r
sdhc_large_chunk_r:
    bsr.w   sl_limit
    cmp.l   %d0,%d2
    ble.b   1f
    move.l  %d0,%d5
1:  jmp     sl_rd_chunk+12

/* All zero at every boot: switch on, no range, counters 0 (the runtime zeroes the bss of an object). */
    .bss
    .balign 4
    .globl sl_off
sl_off:
    .space 4                           /* byte access only (tst.b, C u8 in sdhc.c): the first byte; 0 = on */
    .globl sl_lo
sl_lo:
    .space 4
    .globl sl_hi
sl_hi:
    .space 4
    .globl sl_cnt
sl_cnt:
    .space 16                          /* calls of write, of write that really exceed 64 KiB (limit raised and D2 > 64 KiB), of read, of read that really exceed 64 KiB (limit raised and D2 > 64 KiB) */
