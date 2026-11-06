# Early boot recovery prototype

This is an explicitly selected, unreleased safety proof. Coupled reset fixtures
passed on DN2 1.12 and DT2 1.17; production recovery remains unproved. Do not ship it as a recovery guarantee or a bootscreen
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
before DT2:

```sh
mise exec -- pnpm nx:pilot run core/early-boot-tests:test-device \
  --args="-t '^DN2 1\\.12:' --maxWorkers=1"
```

The reset family starts with `startEmpty: true`, a valid empty selection and
the existing MainPanel reset contract. Startup status genuinely showed epoch 0,
firmware null and no CF runtime. It must reach a safe third boot after two guest
resets, keep the selection, advance DSP frames and respond to SETTINGS/NO.
The exact reset cases passed: DN2 135.92 s total / 123.34 s baseline; DT2
57.15 s / 42.266 s, with an unchanged evidence-retention repeat 57.22 s /
42.372 s. Every original counter/selection, DSP, UI roundtrip, IRQ/input and
lifetime assertion passed. Earlier first/later patched loads over existing
runtime were not rerun; these fixtures do not prove every restart path.

Independent checks passed: 10 normal proof cases (5 per device), 9 existing
DN2 safety cases and 11 existing DT2 safety/crash cases. The isolated DN2 empty
returning-callback control also passed: normal UI, one callback, persisted
counter 1 and 150 additional DSP frames. Thus generic empty coupled startup is
positively checked for that control. The reset stays registered and unskipped.
The full merge gate has not
run; this prototype is not merge-ready. The preserved failures and ledger are in
[finding 115](../../../../docs/findings/115-loadable-bootscreens.md).

The separate `test/boot-early-exception` fixture executes a real illegal
instruction before the original intro installer. Both current-device emulator
cases passed: exact vector/PC, valid retained and persisted CPU fault, two
handler resets, safe third boot with zero modules, DSP/UI/input and lifetime
checks. This proves the guest path under working-card emulator conditions;
hardware exception recovery and blocked-I/O escape are not established.
See the [2026-11-07 follow-up](../../../../docs/handovers/HANDOVER-2026-11-07-early-exception.md).

Storage calls can block before the counter is armed. Prior-crash replay may
flush settings/store data before arming, so preparation is not read-only.
Physical early bypass, initial panel-scan timing and read-only recovery remain
unproved. Global readiness and firmware behavior
are unchanged.
Selection is cached during preparation; successful refresh and later module
loading after transient early reads are also unproved. Counter-write retry
units do not establish read recovery.

Three failed DN2 runs completed their cache pairs 4–6 s after the original
120 s deadline; the former post-cache/transport-hang inference was unsupported.
Only this three-boot reset family now uses a 180 s baseline budget and matching
no-progress watchdog; other fixtures keep their 120 s defaults/environment
behavior. This is a host emulator workload budget, not physical boot timing.
Default-off `DIGI_TRACE_PATCH_BOOT=1` emits timestamped bounded phase markers;
baseline failure artifacts record the pending operation and status phase.

The first DT2 follow-up reached a transient verification/loading screen before
input, causing a false-positive SETTINGS hash change and failed return hash.
A DT2-only 30 s poll now requires this empty-card fixture's known unobscured
main frame before keys. All original assertions remain. Root visually verified
actual SETTINGS in `/private/tmp/bootscreen-dt2-settled-review/`; full outputs
are `/private/tmp/bootscreen-dt2-oracle-{calibration,retained}.log`.
The next safety work is bounded storage and independent bypass, not a player.

## Measured proof cost

The adapter object contains 130 B text and 172 B BSS, with no rodata/data. Its
observed placement is 304 B plus a 16 B hook-tail reservation (14 B code).
The two typed snapshots and counters are proof instrumentation. The shared
runtime adds 520 B text and 7 B BSS against the documentation checkpoint;
rodata/data are unchanged. These are object costs before placement alignment,
not a compressed-flash estimate. Stack use is unmeasured.
