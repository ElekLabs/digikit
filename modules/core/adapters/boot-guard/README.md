# Early boot recovery prototype

This is an explicitly selected, unreleased safety proof. Coupled reset acceptance
is currently red on DN2. Do not ship it as a recovery guarantee or a bootscreen
player. Default patch sets do not include this adapter.

The adapter hooks before the stock intro installer. Assembly preserves the
original skip argument and returns directly to the original installer when
skip is nonzero or the linked `dk_boot_early` symbol is absent. Otherwise it
requests permission from the private runtime gate, records proof snapshots,
calls the linked callback once and continues through the stock installer.
It defines no asset format, SDK contract, service phase or executable card load.

The gate prepares recovery once, requires a valid selection and an owned store
without probe read errors, persists the counter even for an empty selection,
and verifies the actual exception-hook word before permitting the callback.
Module loading remains at the original main-task entry. Late queued-RECORD
checking and normal counter/stability behavior remain there too. Normal UI
exception installation has no extra readback; only early entry checks it.

The linked fixtures are `test/boot-early-probe` (counts callback entries) and
`test/boot-early-reset` (writes the same reset-controller register as the existing
fault fixture). They are fixed image content, not `.dks` providers.

## Checks and current blockers

The uncached owner is `core/early-boot-tests`. Its explicit `patchModules` input
closure builds this adapter without changing default image membership. Run DN2
before DT2; the current reset failure must be diagnosed before another retry:

```sh
mise exec -- pnpm nx:pilot run core/early-boot-tests:test-device \
  --args="-t '^DN2 1\\.12:' --maxWorkers=1"
```

The reset family starts with `startEmpty: true`, a valid empty selection and
the existing MainPanel reset contract. Startup status genuinely showed epoch 0,
firmware null and no CF runtime. It must reach a safe third boot after two guest
resets, keep the selection, advance DSP frames and respond to SETTINGS/NO.
The one DN2 attempt timed out in first prepared baseline setup before those
assertions; no DT2 true-empty reset attempt followed. Earlier first/later
patched loads over existing runtime also timed out and remain unresolved.

Independent checks passed: 10 normal proof cases (5 per device), 9 existing
DN2 safety cases and 11 existing DT2 safety/crash cases. The isolated DN2 empty
returning-callback control also passed: normal UI, one callback, persisted
counter 1 and 150 additional DSP frames. Thus generic empty coupled startup is
positively checked for that control; early-reset recovery is not accepted.
The failed reset stays registered and unskipped. The full merge gate has not
run; this prototype is not merge-ready. The preserved failures and ledger are in
[finding 115](../../../../docs/findings/115-loadable-bootscreens.md).

Storage calls can block before the counter is armed. Prior-crash replay may
flush settings/store data before arming, so preparation is not read-only.
Physical early bypass, initial panel-scan timing, read-only recovery and a real
early CPU exception remain unproved. No timeout or global readiness contract
was relaxed to obtain a pass.
Selection is cached during preparation; successful refresh and later module
loading after transient early reads are also unproved. Counter-write retry
units do not establish read recovery.

The next diagnostic boundary is after the cold worker's cache save: worker
return/teardown, controller completion, patched-audio restore and actor install/
reply. A saved cache pair does not prove boot counters or runtime installation.
Do not add more startup variants or increase timeouts to mask this boundary.

## Measured proof cost

The adapter object contains 130 B text and 172 B BSS, with no rodata/data. Its
observed placement is 304 B plus a 16 B hook-tail reservation (14 B code).
The two typed snapshots and counters are proof instrumentation. The shared
runtime adds 520 B text and 7 B BSS against the documentation checkpoint;
rodata/data are unchanged. These are object costs before placement alignment,
not a compressed-flash estimate. Stack use is unmeasured.
