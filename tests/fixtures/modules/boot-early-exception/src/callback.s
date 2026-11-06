    .text
    .even
    .globl dk_boot_early
/* Same illegal instruction as dyn-crash. The symbol is the exact expected fault PC.
 * The guest's exception handler must record and reset; this fixture never writes RCM.RCR. */
dk_boot_early:
    .short 0x4afc
1:  bra.s 1b
