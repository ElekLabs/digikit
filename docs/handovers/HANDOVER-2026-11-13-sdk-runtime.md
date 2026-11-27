# Handover 2026-11-13: SDK and runtime

Start from docs/plans/sdk-runtime-roadmap.md (the one order) and docs/plans/sdk.md. Main is at b37a79a6.

## State
- Done on main: S1 SDK restructure (PR 416); S2a C hygiene (PR 424); S2b-1 entry point `dk_module`/`dk_module_ctx` and named imports by service (PR 426); S2b-2 all modules on the new contract, the Api table, `dkm_init` and the slot table deleted (PR 433); P7 clock audit (finding 125, PR 422).
- Contract `api = [2, 12]`, runtime 0.6.0. Minor 12 came with the usb-fast merge: `dk_debug_speed_flags`, `dk_usb_bulk_ring`, the `sdhc_large` service (`dk_sdhc_large_open`, `dk_sdhc_large_ctl`). The runtime calls `sdhc_large_range` and `sdhc_large_ctl` through its own adapter header `dkrt_sdhc.h`, like `dkrt_pdw.h`.
- Need recording (sdk.md 5.1): the stub of a missing service accessor `dk_<service>_open` records its provider; when `init` returns DK_E_MISSING the need is the last such provider. This is interim: the registry (runtime-and-services 5.1, 5.2, items R7, R11, R12) will refuse a missing hard need before any module code runs. Open item in 5.2: a module marks an imported service as optional.
- runtime-and-services 5.12 "Service anatomy" (a service is a module with a role) is open and settled with Em during R2, before R7.
- Gate notes: in the three S2b-2 gate runs every failure that was not a real bug was a load symptom ("runtime lifecycle deadline exceeded", "no new boot within 90 s", "emulator closed the connection") and passed when rerun alone. Emulator slot waits were 466 to 977 task-minutes per gate, the biggest gate cost; load reached 13.
- Open: crash-capture (full tier, DT2) failed after S2b-1 and passed once; investigate as a flake. The batch `mise run test:full` after S2b is still to run (Em starts it in ~/src/digi/lanes/sdk-dsp).

## Next
1. S3, SDK builders in `init` and the runtime commit path (sdk.md section 6 and step S3). Package A first: write the interface decisions agreed on 2026-11-13 into sdk.md 6.1, 6.2, 6.3, the S3 step and open item 13, and fix the related lines (line 26 runtime 0.7.0 and api major 3; `dk_setting_add` becomes `dk_settings_add`; the section 10 table gains the `<parent>_add_<kind>` rule and the `_fn` types). The decisions: builders named `<parent>_add_<kind>` (`dk_module_ctx_add_machine`, `dk_module_ctx_add_fx`, `dk_machine_add_page`, `dk_fx_add_page`, `dk_page_add_param`, `dk_page_add_fx_param`, `dk_settings_add`); `dk_page_bind` and `dk_page_bind_local` for params on canvas pages; canvas flags `DK_PAGE_WIDE_CANVAS` and `DK_PAGE_STOCK_OVERLAY`; DSP symbols by name with the toolchain's leading underscore; the `.dkm` header stays 0xa0 and the pair at 0x90/0x94 becomes a keys table (`[uses]` counts, machine keys, FX ids), api major 3, runtime 0.7.0; `DK_MACHINE_ID_<ID>` and `DK_FX_ID_<ID>` in `module_ids.h`; `MAX_RECORDS` 32 is enough (shot about 10 records). Then packages B (runtime commit path), C (packer and dkneeds), D (gen and module:check) in parallel, E (migration by three coders), F (cleanup). The `dk_sample_bank_*` imports go with the bank in S3.
2. S4 templates and docs, including a new `docs/MODULES.md` (module anatomy, entry point, services and handles, declarations and the commit, the load path, contract versions; why each choice), linked from docs/ARCHITECTURE.md; replace or delete the parts of older docs that describe the deleted contract (docs/design/module-sdk.md, digikit-registry.md, digikit-dkm.md).
3. Later in S2b cleanup or R2: the `dk_fw_*` imports expose a firmware name to modules; give them neutral names or move them behind a service.

## Cleanup for Em
- `git -C ~/src/digi/lanes/sdk-design branch -D backup/sdk-design-pre-rebase-1113`
- The merged remote branch: `git push origin --delete lane/sdk-design`
- Lane ~/src/digi/lanes/sdk-fix can be removed now that S2b-2 has merged (if it is not in use).
