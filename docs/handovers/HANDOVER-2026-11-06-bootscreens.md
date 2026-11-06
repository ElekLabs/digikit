# Handover 2026-11-06: loadable bootscreens research

Documentation checkpoint branch: `work/loadable-bootscreens`, in the dedicated
worktree `/Users/em/.codex/worktrees/28d7/digitakt2`. Research began on `main`;
the user authorized moving and committing only these documentation changes here.
Em subsequently authorized the bounded safety-only proof described below in
this same isolated worktree. Existing source-tree `.vscode/` is user work and
remains untouched. No hardware flashing, player, asset I/O or service framework
was authorized or implemented.

Evidence and outstanding acceptance:
[finding 115](../findings/115-loadable-bootscreens.md). DT2 1.17 / DN2 1.12 raw
bytes support 128 × 64 packed 1bpp, configured ~30 Hz, five stock variants, and an
identified 68,172-byte uncompressed MAIN footprint (63,168 bytes of graphics).
This includes shared artwork, excludes shared helper code, and gives no exact
compressed-container saving. The static initial logo's ownership remains open.

The stock boot task uses raw card reads before intro installation. An early
hook is a candidate, not a safe or implemented loader. Today's safe decision and
counter increment happen later at main entry; the exception hook is later again
in the UI loop. Early hangs can bypass them. A bootscreen-only setup/no modules,
read-only storage and an independent bypass need explicit treatment. The stock
skip-intro flag is a bootstrap-supplied console/service bit; no user gesture or
normal setting was established. Preserve it and the completion semaphore.

The sole active architecture plan is
[runtime and services](../plans/runtime-and-services.md), a draft with nothing
built. It proposes dependency-loaded `.dks` providers containing adapters/hook
tables, while retaining sites needed before main entry in the image. Do not
silently add an early provider phase or move the full loader. The user prefers
future on-demand services; decide explicitly between a feature adapter with a
minimal resident early gate and a generic early phase after proving its safety.
Keep feature parsing out of the runtime.

## Safety-only implementation checkpoint

The opt-in `core/boot-guard` adapter has a hash-resolved before-hook on the
stock intro installer, with its skip argument and original installer preserved.
It calls a private once-only runtime recovery gate only when a linked optional
test callback exists. Preparation and late module loading are separate; no
modules load early. The gate requires owned storage, a valid selection and a
successful counter write even for an empty selection, and verifies the actual
exception-hook word. Default patch-set membership and the SDK are unchanged.

The runtime's normal void exception installation path is unchanged; checked
readback is private to early entry. The late queued-RECORD check remains. This
does not establish initial-scan or physical bypass timing. Prior-crash replay
can flush settings before arming, and raw storage calls remain blocking.
Early preparation also caches the selection. Successful selection refresh and
later module loading after transient early read errors are not proved; the
counter retry units cover failed writes only.

Checks actually completed:

- `mise exec -- pnpm nx:pilot run core/runtime:rust-tests`: 55 passed.
- Current DN2/DT2 port resolution, generation and `mise run check:layers` passed.
- CLI parser units: 8 passed; validated first-coupled stock-base units: 2 passed.
- Test-support host tests: 5 passed; test-support and regression type checks passed.
- Final `mise run test:plan` completed; `mise run test:changed` passed all 11 tasks
  (48.4 s). Formatting and diff checks passed. The full merge gate has not run.
- Current non-reset proof families: 10 passed (5 on each device), 414.13 s,
  before the isolated empty-control addition. The compiled runtime is unchanged.
- Existing DN2 safe-mode/torn-selection file: 9 passed, 789.95 s, no early adapter.
- Exact existing DT2 safety files, with `DIGIKIT_TIER=full`, one worker and
  `-t '^DT2 1\.17:'`: 11 passed (9 safe-mode/torn-selection + 2 crash-capture),
  597.16 s. Crash-capture is DT2-only; there is no DN2 crash-capture result.
- True-empty DN2 returning-callback control: 1 passed, 50.31 s, including empty
  startup observation, normal UI, one callback, persisted counter 1 and 150 more DSP frames.
- An earlier new-proof iteration passed all six DN2 cases and five DT2 cases;
  its DT2 reset failed a readiness-contract check. Those results do not accept
  the strengthened current revision. An owner-wide legacy title filter also
  selected unintended cases and was stopped; it is not a passed check.

The strengthened reset case and paired empty control use the existing
`relaxFs: true` reset contract. The reset case requires DSP frame growth and an actual
SETTINGS/NO screen response. Record export pauses the daemon, so it explicitly
resumes it before those checks and drains/pauses in cleanup. No global harness
contract or timeout was changed. Its two latest DN2 attempts, concurrent and
then alone, both failed **inside baseline reload before those assertions**:
190.56 s / 199.30 s total, roughly 130 s reload deadline. The no-progress
watchdog could not obtain asynchronous status. The saved paused/ready epoch is
pre-reload, not a post-failure DSP observation. Do not call this a DSP stall or
hardware failure, and do not repeat it blindly.

A separately approved scenario then seeded the **initial** fixture
with a valid empty selection. It uses `h.ready(true)` without a second `h.boot`
or the normal-only `h.first` helper. That attempt also failed during initial
baseline establishment (133.24 s total, about 130 s reload), before any body
assertion. It obtained neither an exact reset count nor a completed lifetime
proof. **Correction:** `Session.start` already loads the positional stock image
before the fixture, so this was a first patched load over stock runtime.
First and later patch-load failures remain open; that attempt was not an empty
coupled power-on proof.

The approved `startEmpty` seam now genuinely omits the startup CF image while
retaining automatic coupled audio, explicit profile identity and all ready
inputs. It is restricted to reusable coupled sessions. The controller reads
and hashes the configured stock profile for the first prepared load, preserving
the DSP diff and later retained-base logic. Original numeric epoch, prepared
identity and post-load coupled-audio checks remain strict. No engine, reset,
snapshot format, readiness contract or timeout changed.

The single true-empty DN2 reset attempt recorded epoch 0 / firmware null / no
CF runtime, then failed during first prepared baseline setup (132.69 s total,
about 130 s reload); none of its safe/counter/DSP/UI body assertions ran. No
DT2 true-empty reset attempt followed. The distinct DN2 returning-callback
control passed under the same empty/card/MainPanel configuration, so generic
empty startup is positively checked while early-reset acceptance remains red.
The reset test remains registered and unskipped.

Preserved latest failure evidence:

- `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-session-mw5OWR`
- `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-early-boot-reset-recovery-dn2-sVJR3r`
- Initial-seeded case: `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-session-hvX5eC`
- Initial-seeded case: `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-early-boot-reset-recovery-dn2-A4n70I`
- True-empty case: `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-session-fvVyqx`
- True-empty case: `/private/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-early-boot-reset-recovery-dn2-YhsOpd`

The nested uncached owner is `core/early-boot-tests`. Its `patchModules` metadata
builds only the explicit core closure (including `core/boot-guard`) and covers
both linked fixtures in its inputs; each actual image selects one callback or
none. The parent `core/runtime-tests` metadata is unchanged. Exact-file
iteration after those prerequisites are built is, for example:

```sh
mise exec -- pnpm --filter @digi/regression test:runtime \
  ../../modules/core/runtime/tests/early-boot/early-boot.device.test.ts \
  -t '^DN2 1\.12: early boot reset recovery' --maxWorkers=1
```

Built-object delta: common runtime +520 B text / +7 B BSS; rodata/data unchanged.
The adapter is 130 B text / 172 B BSS, no rodata/data; the observed plan reserves
304 B plus a 16 B hook tail (14 B code), 320 B total. These include private
proof instrumentation. Stack and compressed-container flash delta are unknown.
This remains an experimental checkpoint on `work/loadable-bootscreens` in the
dedicated worktree. No push, PR or merge; not merge-ready. Reset acceptance is
red and the full merge gate has not run.

Next steps, in order:

1. Diagnose the post-cache lifecycle before another reset run. The unique run's
   cache pair is written after cold ready/DSP-quiet/save/export; it does not
   prove actor installation or boot counters. Add bounded markers around worker
   return/teardown, controller completion, patched-audio restore and actor
   install/reply. No more startup variants, timeout increases or CF-only substitutes.
   The generic empty control passed. Finish strengthened DN2 reset acceptance
   before a DT2 true-empty reset attempt; the independent legacy suites are done.
2. Review independent bypass and bounded storage against the active service
   plan. Stock block reads wait on mutex/semaphores; a caller timeout alone
   cannot recover a blocked call. Resolve the pre-arm crash/settings writes and
   all pending safety cases in finding 115 without adding a provider phase.
3. Complete the owner's safety acceptance before building a player. Prove a common
   completed framebuffer boundary too: today's setPixel observer misses memcpy
   frames. Existing safety tests are reuse points, not proof of the early phase.
4. Report permanent flash/core-stub cost; provider file/code/rodata/data/BSS/stack;
   asset disk size, retained RAM, staging, framebuffers, decoder and zero-provider
   path. The stock footprint does not predict adapter cost. No arbitrary estimates.
5. Build the asset converter/player only after those boundaries pass, then add
   workbench packaging/upload/selection. Canvas plus SVG/PNG, PNG/timing/APNG
   export and device holds/XOR/RLE/raw fallback are proposals, not a format.
   Preserve bitmap flags/orientation/ownership. Any future cleanup must wait for
   callback return and removal of all stock pointers into provider memory;
   current modules remain loaded for the session and no unload is promised.
6. Give a ceiling and one paired A/B before performance claims. Neither device
   throughput nor custom-player costs were measured in this session.

Reuse the [existing runtime safety suites](../findings/115-loadable-bootscreens.md#acceptance-still-pending)
through `core/runtime-tests:test-device`. They run against emulator daemons;
they do not establish held-RECORD hardware timing or a pre-main hook's safety.

The research validation remains firmware queries, raw-byte/listing comparisons
and scalar range checks. Implementation validation is the partial ledger above;
the strengthened recovery proof is red. No hardware recovery or zero-risk
guarantee is established; bootloader STARTUP is a separate path.
