    .text
    .globl peer_sample_boot
peer_sample_boot:
    jsr     dk_peer_sample_preload
    jmp     peer_sample_boot_tail
