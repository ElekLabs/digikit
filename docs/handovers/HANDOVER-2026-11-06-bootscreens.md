# Handover 2026-11-06: loadable bootscreens research

Documentation checkpoint branch: `work/loadable-bootscreens`, in the dedicated
worktree `/Users/em/.codex/worktrees/28d7/digitakt2`. Research began on `main`;
the user authorized moving and committing only these documentation changes here.
No code, SDK, manifest, firmware patch or emulator work was authorized or performed.
Existing source-tree `.vscode/` is user work and remains untouched. All
implementation still requires agreement and a dedicated implementation worktree.

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

Next steps, in order:

1. First do read-only recovery/phase design with Em against the active service
   plan: decide the independent bypass, bounded storage/failure path and adapter
   architecture. Stock block reads wait on mutex/semaphores; a caller timeout
   alone cannot recover a blocked call. Account for all pending safety cases in
   finding 115. Do not create a new SDK, phase or provider format incidentally.
2. Begin implementation only after agreement and in a dedicated worktree.
3. Make a bounded safety-only proof on DN2, then DT2, using the owner's harness
   and acceptance target before building the feature player. Prove a common
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

Validation so far is static only: firmware queries, raw-byte/listing comparisons,
scalar table/asset range checks and documentation checks. No hardware recovery
or zero-risk guarantee is established; bootloader STARTUP is a separate path.
