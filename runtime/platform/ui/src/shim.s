| core/ui: the assembly shim. ColdFire, gas syntax, mcpu=54418.
|
| dk_make_sp(out, parent, view): the firmware's shared_ptr<View> constructor takes `out` in a0 and (parent, view) on the
|   stack; this adapts the cdecl call from C. It makes the control block (use count 1) and registers a detach callback on the
|   view that clears the shared_ptr at parent+0x90. Core/ui passes a parent that is a zeroed heap block, so that clear changes
|   nothing, and keeps the returned shared_ptr for the session: the firmware then never frees the view.
    .text
    .globl dk_make_sp
dk_make_sp:
    move.l  4(%sp),%a0
    move.l  12(%sp),-(%sp)          | view
    move.l  12(%sp),-(%sp)          | parent (8(%sp) on entry, shifted by the first push)
    jsr     cf.string.fn.shared_ptr.make
    addq.l  #8,%sp
    rts

| dk_hook: the hook at the end of the SETTINGS rebuild (cf.menu.hook.settings_list.tail), entered by `jmp`. Live registers: a2 = the
|   settings menu (`this`), d4 = the saved selection, sp = the rebuild's frame; d0/d1/a0/a1 are dead. It calls the optional
|   dk_app_settings_rows(settings_menu) of the app (an optional link: 0 when no app is in the image) then the rows that modules registered (ui_settings_rows_tail), and returns to the rebuild
|   through the hook tail, where the displaced instructions run.
    .globl dk_hook
dk_hook:
    move.l  #dk_app_settings_rows,%d0
    jeq     1f
    move.l  %d0,%a0
    move.l  %a2,-(%sp)
    jsr     (%a0)
    addq.l  #4,%sp
1:  move.l  %a2,-(%sp)
    jsr     ui_settings_rows_tail   | the rows of modules (dk_ui_settings_row); C code keeps d2-d7 and a2-a6
    addq.l  #4,%sp
    jmp     _dk_tail
