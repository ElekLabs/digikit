| project.dks, the hooks of the stock project paths (the same code on every device that has the functions: a hook is bound only where the port file lists it). They are `.dks` hooks (`HOOKS` of the service, listed in the port file of the firmware they run on). Each runs in place of 6 or 8 bytes of a stock function: the
| setter's entry, and for the save, the paste and the two clears the exit (just before the rts). The paste and the clears are jobs of the firmware's BgWorker thread that finish their card write and
| listing refresh inside the job, so only their exit tells the service that the slot record is final; the service reads it in the UI task tick. Each
| only RECORDS into `DKRT_PROJECT_PENDING` (`dkrt_svc_project::engine::Pending`, offsets below); the service acts in its tick, in the UI task. None calls anything or waits. Each leaves by
| `jmp` to its tail (`project_t_*`, `[object.externs]` of module.toml): the displaced instructions of the site, then the rest of the stock code.
|
| Pending: +0 slot_val, +4 slot_flag, +8 save_slot, +12 save_flag (longs; a flag is written after its slot), +16 clear bitmap and +32 paste bitmap (16 bytes each, byte slot/8, bit slot%8).
| Registers (tools/coldfire/liveness.py, strict): the setter's first instruction writes D1 and the flags, so the hook may change them. Every other routine saves the registers it uses and the
| condition codes: strict liveness proves none of them dead (it assumes all live at a return and at a call), and a few instructions cost less than a wrong assumption.

    .text
    .balignw 4, 0x4e71

| Set bit D1 (a slot 0 to 127) of the bitmap at \base. D0 and A1 are scratch.
    .macro project_set_bit base
    move.l  %d1,%d0
    lsr.l   #3,%d0
    lea     \base,%a1
    adda.l  %d0,%a1
    moveq   #7,%d0
    and.l   %d1,%d0
    bset.b  %d0,(%a1)
    .endm

| Save D0, D1, D2, A0, A1 and the condition codes in a 24 byte frame, and give them back.
    .macro project_save
    lea     -24(%sp),%sp
    movem.l %d0/%d1/%d2/%a0/%a1,(%sp)
    move.w  %sr,%d0                     | the low byte is the condition codes (move from CCR is not in this ISA)
    move.l  %d0,20(%sp)
    .endm
    .macro project_restore
    move.l  20(%sp),%d0
    move.w  %d0,%ccr
    movem.l (%sp),%d0/%d1/%d2/%a0/%a1
    lea     24(%sp),%sp
    .endm

| The slot setter `void set(int slot)` (sites and check values: devices/fw/project.toml). 4(sp) = slot (-1: no project). The displaced `mvs.b (slot byte).l,D1` writes D1 and the flags first. The
| call with the slot that is current is recorded too: the stock setter ignores it, the service does.
    .globl project_hook_slot
project_hook_slot:
    move.l  4(%sp),%d1
    move.l  %d1,DKRT_PROJECT_PENDING
    moveq   #1,%d1
    move.l  %d1,DKRT_PROJECT_PENDING+4
    jmp     project_t_slot

| The save exit, before the rts of the save. D0.b is nonzero when the card write succeeded, D3 = the slot (0x80 on the mirror save). Only a
| successful save of a slot 0 to 127 is recorded. D0 and D3 are untouched.
    .globl project_hook_save
project_hook_save:
    project_save
    tst.b   3(%sp)                      | the saved D0 (D0 itself now holds the condition codes); its low byte is at +3
    beq.b   1f
    cmpi.l  #127,%d3
    bhi.b   1f
    move.l  %d3,DKRT_PROJECT_PENDING+8
    moveq   #1,%d1
    move.l  %d1,DKRT_PROJECT_PENDING+12
1:  project_restore
    jmp     project_t_save

| The exit of the clear callback of one project, after the card write and the listing refresh. SP is the entry SP - 0x38 (+ 24 of the frame here), so the job
| (the entry's 4(SP)) is at 0x3c(SP) and the slot is **job. The stock function always returns 1; the slot is recorded whatever the card write did.
    .globl project_hook_clear_one
project_hook_clear_one:
    project_save
    movea.l 84(%sp),%a0                 | 0x3c + 24
    movea.l (%a0),%a0
    move.l  (%a0),%d1
    cmpi.l  #127,%d1
    bhi.b   1f
    project_set_bit DKRT_PROJECT_PENDING+16
1:  project_restore
    jmp     project_t_clear_one

| The exit of the clear callback of several projects. SP is the entry SP - 0x34, so the job is at 0x38(SP) (80 here). **job is a vector of 32-bit slots (begin
| at +0, end at +4), read again here: the stock loop has left A2 and D5 at its end. Every slot is recorded whatever the result in D0.b. At most 129 entries are read
| (a vector that is longer, or ends before it begins, is left alone).
    .globl project_hook_clear_many
project_hook_clear_many:
    project_save
    movea.l 80(%sp),%a0                 | 0x38 + 24
    movea.l (%a0),%a0
    movea.l (%a0),%a0
    move.l  4(%a0),%d2
    movea.l (%a0),%a0
    move.l  %d2,%d1
    sub.l   %a0,%d1
    cmpi.l  #516,%d1
    bhi.b   9f
2:  cmp.l   %a0,%d2
    bls.b   9f
    move.l  (%a0)+,%d1
    cmpi.l  #127,%d1
    bhi.b   2b
    project_set_bit DKRT_PROJECT_PENDING+16
    bra.b   2b
9:  project_restore
    jmp     project_t_clear_many

| The paste exit, before the rts: D0.b is nonzero when the copy reached the card write (the write call's own result is discarded and not returned), D6 = the destination slot (also 12(A6)). A paste that did not reach the card write records nothing. D0 and D6 are untouched.
    .globl project_hook_paste
project_hook_paste:
    project_save
    tst.b   3(%sp)                      | the saved D0 (D0 itself now holds the condition codes); its low byte is at +3
    beq.b   1f
    move.l  %d6,%d1
    cmpi.l  #127,%d1
    bhi.b   1f
    project_set_bit DKRT_PROJECT_PENDING+32
1:  project_restore
    jmp     project_t_paste
