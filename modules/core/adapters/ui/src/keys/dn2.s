/* core/ui: the key ids of the firmware's key events for this device (`[input.view_keys]` of the device file, through build/gen/ui-dn2.inc).
 * Layout (words): yes, yes2 (0: none), no, right. ui.c compares the id of a key event with them. */
    .include "ui-dn2.inc"
    .section .rodata
    .even
    .globl dk_ui_view_keys
dk_ui_view_keys:
    .long UI_KEY_YES, UI_KEY_YES2, UI_KEY_NO, UI_KEY_RIGHT
