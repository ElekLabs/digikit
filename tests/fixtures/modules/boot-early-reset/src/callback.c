/* Same MCF5441x SOFTRST register as the existing dyn-fault fixture. */
void dk_boot_early(void)
{
    *(volatile unsigned char *)0xEC090000 = 0x80;
    for (;;) {
    }
}
