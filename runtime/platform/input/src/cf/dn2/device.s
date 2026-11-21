/* core/input: the device object. The input backend and the panel's key codes are the device file's ([input] backend, [panel]) through
 * build/gen/input-dn2.inc, which `patchkit gen` writes. Layout (words): backend, linear channels, encoders, number of exceptions, then 8 x
 * (code, channel, bit) for the codes that are not linear, then the number of button channels (the button decoder's tables). */
    .include "input-dn2.inc"
    .section .rodata
    .even
    .globl input_cfg
input_cfg:
    .long INPUT_BACKEND, PANEL_LINEAR, PANEL_ENCODERS, PANEL_EXC_N
    .long PANEL_EXC_CODE_0, PANEL_EXC_CH_0, PANEL_EXC_BIT_0
    .long PANEL_EXC_CODE_1, PANEL_EXC_CH_1, PANEL_EXC_BIT_1
    .long PANEL_EXC_CODE_2, PANEL_EXC_CH_2, PANEL_EXC_BIT_2
    .long PANEL_EXC_CODE_3, PANEL_EXC_CH_3, PANEL_EXC_BIT_3
    .long PANEL_EXC_CODE_4, PANEL_EXC_CH_4, PANEL_EXC_BIT_4
    .long PANEL_EXC_CODE_5, PANEL_EXC_CH_5, PANEL_EXC_BIT_5
    .long PANEL_EXC_CODE_6, PANEL_EXC_CH_6, PANEL_EXC_BIT_6
    .long PANEL_EXC_CODE_7, PANEL_EXC_CH_7, PANEL_EXC_BIT_7
    .long PANEL_BUTTON_CHANNELS
