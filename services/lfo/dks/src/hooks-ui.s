| lfo.dks, the hooks of the UI task (both devices, the same code): the name of a shape on the MOD page, and the tile vector of its graph. The LFO tick runs no code of this file: the waves are data of `core/lfo-table`.
| They are `.dks` hooks (`HOOKS` of the service, listed in the port file of the firmware it runs on), not interrupt slots: the UI task runs them, one at a time. Each runs in place of the first
| 6 bytes of a stock function and leaves by `jmp` to `lfo_x_*`, the end of its site (`[object.externs]` of module.toml, the hooks of ports/fw/lfo.toml). With no shape loaded, the stock code
| runs unchanged. Register facts (docs/findings/149-lfo-waveforms.md, "The panel hooks", from `tools/coldfire/liveness.py` on the stock bytes): at the entry of each function the displaced
| instructions leave d0 written before they are read, and the flags dead; every other register the routines use is saved.

    .text
    .balignw 4, 0x4e71

| The name formatters `fmt(int value, char *buf)`: sprintf(buf, "%s", name[value >> 8]) for the seven stock waves, sprintf(buf, "ERR") above them. Entry: 4(sp) = value, 8(sp) = buf.
| The displaced instructions are `link.w a6,#-0x1c; moveq #6,d0` (6 bytes). For a value of 0x700 to 0xeff (the wave numbers 7 to 14) with a loaded shape, `dk_lfo_name(value, buf, width)` writes
| the name (`width` characters at most) and the NUL, and the routine returns the number of characters in d0, as the stock sprintf does. Any other value, and a number with no shape, runs the stock code.
| d1, a0 and a1 (the callee's scratch registers) are saved around the call, so the routine leaves what the stock code leaves untouched until its own calls.
    .macro lfo_name_hook entry, width, exit
    .globl \entry
\entry:
    move.l  4(%sp),%d0
    cmpi.l  #0x700,%d0
    blt.b   9f
    cmpi.l  #0xeff,%d0
    bgt.b   9f
    lea     -12(%sp),%sp
    movem.l %d1/%a0/%a1,(%sp)           | value now at 16(sp), buf at 20(sp)
    move.l  #\width,-(%sp)              | argument 3
    move.l  24(%sp),-(%sp)              | argument 2: buf
    move.l  24(%sp),-(%sp)              | argument 1: value
    jsr     dk_lfo_name
    lea     12(%sp),%sp
    movem.l (%sp),%d1/%a0/%a1
    lea     12(%sp),%sp
    tst.l   %d0
    beq.b   9f
    rts
9:  link.w  %a6,#-0x1c                  | the displaced instructions
    moveq   #6,%d0
    jmp     \exit
    .endm

    lfo_name_hook lfo_ui_name4, 4, lfo_x_name4
    lfo_name_hook lfo_ui_name3, 3, lfo_x_name3

| The vector pick of the LFO page's cell (the shape graph, docs/plans/extension-model.md E6 item 1b). The site is `addi.l #TABLE,D6` (6 bytes) before `move.l D6,-(SP); jsr (A5)`:
| D6 = 12 * the clamped wave, the base of the firmware's RAM table of seven tile vectors is added, and the accessor picks a tile from the vector. The cell's frame (A6) holds the wave
| as its argument at 0x18(A6) (unclamped: the cell clamps a copy into D5) and the byte the cell reads for the number of tiles drawn at -0x11(A6). The vtable word of the tiles' class is
| read from the stock vector: the tail's first instruction is the displaced `addi.l #TABLE,D6` (checked by its opcode word; its immediate is the table), so TABLE + D6 is the stock
| vector, whose begin is the first stock `Bitmap` and whose first word is the vtable (D6 itself stays, the tail adds it). For a wave of 7 to 14 whose shape has a published graph, `dk_lfo_graph(wave, vtable, &flag)` returns the service's
| vector and writes the unipolar byte; D6 gets the vector instead of table + 12 * 6 (the RAND tiles). Every other case goes to the tail: the displaced `addi` (the table address is
| firmware data: bind generates the tail from the user's image), then the stock code.
| Registers (strict liveness, docs/findings/149-lfo-waveforms.md "Graph"): only the flags are dead at the exit; D0, D1, A0 and A1 are saved and restored around the call, D6 is the output
| (the stock `addi` writes it too). The flags: the stock `addi` of a positive table address to a small D6 leaves N, Z, V, C and X clear, so the shape path clears the condition code register,
| and the tail path runs the `addi` itself. The hook never runs in interrupt context: the status register is read first, and a non-zero interrupt mask goes to the tail.
    .globl lfo_ui_graph
lfo_ui_graph:
    lea     -16(%sp),%sp
    movem.l %d0/%d1/%a0/%a1,(%sp)
    move.w  %sr,%d0
    andi.l  #0x700,%d0
    bne.b   9f
    move.l  0x18(%a6),%d0               | the unclamped wave
    subq.l  #7,%d0
    cmpi.l  #7,%d0                      | unsigned: 7 to 14
    bhi.b   9f
    lea     lfo_t_graph,%a0             | the tail starts with the displaced `addi.l #TABLE,D6` (0x0686, then the immediate, 2 byte aligned)
    moveq   #0,%d0
    move.w  (%a0),%d0
    cmpi.l  #0x0686,%d0
    bne.b   9f                          | not that instruction: no vtable, no graph
    moveq   #0,%d0
    move.w  2(%a0),%d0
    swap    %d0
    move.w  4(%a0),%d0                  | TABLE
    add.l   %d6,%d0                     | TABLE + 12 * the wave: the stock vector, without changing D6 (the tail adds it)
    movea.l %d0,%a1
    movea.l (%a1),%a0                   | its begin
    move.l  %a0,%d0
    beq.b   9f
    move.l  (%a0),%d0                   | the vtable word of its first Bitmap (the tiles' class)
    beq.b   9f
    pea     -0x11(%a6)                  | argument 3: the unipolar byte
    move.l  %d0,-(%sp)                  | argument 2: that vtable word
    move.l  0x18(%a6),-(%sp)            | argument 1: the wave
    jsr     dk_lfo_graph
    lea     12(%sp),%sp
    tst.l   %d0
    beq.b   9f
    cmpi.l  #1,%d0                      | the sentinel of an added wave with no shape (`STOCK_SIN`, not an address)
    beq.b   8f
    move.l  %d0,%d6
    movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    move.w  #0,%ccr
    jmp     lfo_x_graph
| A wave with no shape draws the stock SIN tile because the LFO plays the stock sine for it (`core/lfo-table`): D6 = 12 (the second vector), the tail adds TABLE (the SIN vector). The stock flag byte set for the clamped wave stays right (SIN and RND are both bipolar).
8:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    moveq   #12,%d6
    jmp     lfo_t_graph
9:  movem.l (%sp),%d0/%d1/%a0/%a1
    lea     16(%sp),%sp
    jmp     lfo_t_graph

| The firmware words of the WAVE maximum (`setup` of hooks.rs): the stock parameter table and the stock rows of the WAVE parameters of the three LFOs. `bind` resolves the four names
| (`[object.externs]` of module.toml; anchors of ports/fw/lfo.toml and param.toml). The service writes the maximum into the RAM copy of the table at run time (docs/plans/extension-model.md E6c).
    .data
    .balign 4
    .globl DKRT_LFO_FW
DKRT_LFO_FW:
    .long   lfo_wave_table, lfo_wave_row1, lfo_wave_row2, lfo_wave_row3
