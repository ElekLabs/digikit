# Handover 2026-11-14: SDK S3 and address map P1

Start from docs/plans/sdk-runtime-roadmap.md (the one order). Main is at cacb7cc1.

## State
- S3 merged (PR 443): modules declare with builder calls in init; the runtime collects and commits all at once or refuses; module.toml [[machine]]/[[fx]] ids with an optional `dsp` prefix for existing DSP symbols; per-device [uses.dt2]/[uses.dn2] (rows and record_bytes) written by module:check; module_ids.h holds indexes and the packer computes keys from the packed name (aliases made with `pack --name` get their own keys); one version, the runtime semver 0.7.0 (`since` strings, no contract table); MODULE_COMMIT and MODULE_LATCH log events; the sample bank, the dsp-ddr/lp0 proofs and the old register path are gone; the declaration macros live only in sdk/patched-in/include for the two patched-in twins.
- P1 merged (PR 444): address map v1 (docs/ADDRMAP.md): packages/dkaddrmap, `patchkit addr`, ship-safe and dev-only maps, names in dklog crash, fastcov, snapread, cfprof, pcdump_names.
- Work rule now in use: agents build with host checks only; device verification runs once at the end as one gate; a slice proof only for a new design.
- Gate notes: the S3+P1 gate took 101 min with 1640 task-minutes of emulator slot waits; 3 of 20 failures were load symptoms ("no new boot within 90 s") that passed alone. A menu test failed because lanes share out/ and a pack was reused across lanes (fixed with DIGI_TRANSFER_DIR per run).

## Next
1. S4: templates are on builders; write docs/MODULES.md (what a module is, the entry point, services and handles, builder calls and the commit, the load path, versions; why each choice), linked from docs/ARCHITECTURE.md; rewrite docs/MODULE-AUTHORING.md, sdk/README.md and the digikit-module skill on the builders; replace or delete the parts of older docs about the removed contract (docs/design/module-sdk.md, digikit-registry.md, digikit-dkm.md). Can run in parallel with R4.
2. R4 spike (static only): can SHARC hooks move into a `.dks`.
3. S3b: delete the patched-in content path (roadmap row).
4. Test-speed items to place in docs/plans/test-speed.md: retry a device target once on known load symptoms (logged); order device tasks longest first from the gate history; write a JSON gate result (failed targets, symptom, log path).
5. Smaller follow-ups: tools/patchkit/pk_committed.py is new Python (module:check is still Python); P8: the crash record should carry module load bases; the Nx daemon key includes patchkit, so a patchkit edit rebuilds the daemons (about 13 min); `tools/nx/modules.mjs` pack-aliases is used again since the alias revert (check nothing stale remains).

## Cleanup for Em
- Merged lane branches: lane/sdk-design (remote too), sdk/s3, sdk/s3a-interfaces, sdk/s3b-runtime, sdk/s3c-packer, sdk/s3d-gen, sdk/s3e-machines, sdk/s3e-fixtures, sdk/s3f-cleanup, sdk/s3-fix-host, gate/s3-p1, prof/p1-addrmap, backup/sdk-design-pre-rebase-1113, docs/s2b-done; remote: sdk/s3-pr, prof/p1-pr, docs/s2b-done, lane/sdk-design.
- Lanes ~/src/digi/lanes/sdk-fixtures and ~/src/digi/lanes/sdk-gen were used for S3 and P1 (warm; keep or remove).
