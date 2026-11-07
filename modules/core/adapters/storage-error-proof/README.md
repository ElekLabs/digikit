# Private returned storage errors

`core/storage-error-proof` is a test-only opt-in function-entry shim for the
existing block read/write drivers on DT2 1.17 and DN2 1.12. It is distinct from
`core/storage-fault-proof`, the DT2 raw IRQSTAT observer. Neither is a default
patch module.

Exactly one pure-data `test/boot-io-*` fixture must supply `BOOT_IO_MODE`.
Linking the adapter alone intentionally fails for the missing required symbol;
configurations conflict with one another. Modes: pass (0), selection B once (1),
selection A and B once each (2), first 2,560-byte store read once (3), persistent
store write errors (4), and store header A read once (5). Before BOOT_GUARD_ATTEMPTS becomes nonzero, all operations pass without counters.
Matching is restricted
to request starting sectors in the runtime store; other requests pass untouched.
The payload fixture uses a 2,166-byte DKBS rounded to 2,560 bytes and excludes
512-byte headers and the 5,120-byte c-demo read.

The custom entry shim preserves D0/D1/A0/A1 and caller-owned arguments. A C
decision either resumes the generated displaced-prologue tail or returns -1 to
the original caller. It does not call storage, inspect/mutate buffers, access
status MMIO, delay, or spin. Aligned private counters record in-store matching
and rejected operations plus the last rejection. It models returned driver
errors, including a persistent read-only error model; it does not prove actual
DMA errors, hardware write protection, bounded blocked I/O, or cancellation.

The device-only owner `core/storage-error-tests` registers eight cases on both
current images: pass, one-copy, both-copies, payload, write-error, probe,
held-write-error and safe-once-write-error. The last two reuse the persistent
write-error configuration. The seed has valid empty A sequence 1 and selected
c-demo B sequence 10; the safe-once case sets B's safe flag. Every case records
selection copies, boot/provider/guard/scene/fault/card observations, actual store
write sectors and intro evidence before assertions. Write-error cases require
an empty actual store-write sector list. Held RECORD skips intro capture and is
released before the existing UI checks.

DN2 one-copy passed after the bounded early-selection fix; updated DT2 pass
also passed. Eleven parent-selected expanded cases also passed, bringing
updated-runtime coverage to 13/16 registered cases; updated DN2 pass, DT2 both-copies and DT2
probe remain unrun. All selected cases passed the existing UI/input/DSP and
lifetime checks, with actual empty store-write lists for write-error cases.
This returned-status model makes no physical write-protection/DMA-stall claim. Blocked I/O and persistent late-read-error
policy remain open.
