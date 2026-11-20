/* core/ui: the key ids of the firmware's key events for this device (`[input.keys]` and `[input.key_flags]` of the device file, through build/gen/ui-st.inc).
 * Layout (words): YES, LEVEL_PUSH (the second YES key), NO, RIGHT, then the press and release flag bits. ui.c compares the id of a key event with them. */
    .include "ui-st.inc"
    .section .rodata
    .even
    .globl dk_ui_view_keys
dk_ui_view_keys:
    .long UI_KEY_YES, UI_KEY_YES2, UI_KEY_NO, UI_KEY_RIGHT, KEY_FLAG_PRESS, KEY_FLAG_RELEASE
