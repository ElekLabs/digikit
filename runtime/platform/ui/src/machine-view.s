| core/ui: the machine selection view. The firmware builds the view's item list once, from the machine list getter, and keeps the list; the machines service changes that list at run time and builds it
| again. The hook remembers the view (the one place that has it: the view's constructor calls the builder once, at boot) in `ui_machine_view`, a word the service reads (the info entry `machine_view`).
| It is resident because the constructor has run before the service file loads.
|
| `ui_machine_view_hook` is the entry of the item builder `cf.machine.fn.view.build(view)` (view at 4(sp)). The macro keeps every register; the displaced instructions run in the tail.

    .include "dk_fw_machine.inc"

    .text
    .globl ui_machine_view_hook
ui_machine_view_hook:
    dk_fw_machine_view_build_hook ui_machine_view_store, _ui_view_next

| C: void ui_machine_view_store(u32 view).
ui_machine_view_store:
    move.l  4(%sp),%d0
    move.l  %d0,ui_machine_view
    rts

    .section .rodata
    .balign 4
    .globl ui_machine_view_info
ui_machine_view_info:               | the info entry `machine_view`: what the machines service needs to build the view's list again
    .long   ui_machine_view         | 0 the variable that holds the view (0 until the firmware made it)
    .long   _ui_view_build          | 1 the function that builds the view's items: build(view)

    .bss
    .balign 16                          | a cache line of its own: no code shares the line of a variable the CPU writes
    .globl ui_machine_view
ui_machine_view: .space 16
