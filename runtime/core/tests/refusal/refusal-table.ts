/**
 * The refusal fixtures of the loader and the boot plan (docs/plans/runtime-and-services.md section 8, item R11): for every load status of `[load.status]`
 * one proof that a module is refused with that code (must fail) and one that the same module loads without the fault (must pass), or the reason why a code
 * has no module-file fixture. `refusal-table.test.ts` fails when a code has no row, when a row names a test that is gone, and when an exempt code gives no reason.
 *
 * A proof is a device test (`*.device.test.ts`: a packed or patched `.dkm` through the real loader, asserting the module's state and code) or, for a code that no
 * file can cause, a host test of the layer that returns it. `kind` says which.
 */
export interface Proof {
  /** The file, relative to the repository root. */
  file: string;
  /** A substring of the title of the test in that file. */
  test: string;
}

export type Row =
  | { fail: Proof; pass: Proof }
  | {
      /** `reserved`: the code is never returned. `not-a-load`: a module file cannot cause it (a store, selection, card or files result). `host`: only a host test of the layer reaches it. `open`: no fixture yet. */
      kind: 'reserved' | 'not-a-load' | 'host' | 'open';
      reason: string;
      /** Host tests (`kind: 'host'`) or other tests that return the code. */
      covered?: Proof[];
    };

const REFUSAL = 'runtime/core/tests/refusal/refusal.device.test.ts';
const CAPACITY = 'runtime/core/tests/capacity/capacity.device.test.ts';
const COMMIT = 'runtime/core/tests/capacity/commit.device.test.ts';
const ASSEMBLY = 'runtime/core/src/rs/src/package/loader/tests/assembly_tests.rs';
const IMAGE_RS = 'runtime/core/abi/src/image.rs';
const SAFE = 'runtime/core/tests/boot/safe-mode.device.test.ts';
const COLLECT = 'runtime/core/src/rs/src/package/collect/tests.rs';
const MACHINES_BUILD = 'services/machines/tests/build.rs';
const CAP_RS = 'runtime/core/sdk/src/capacity/tests.rs';
const STORE_RS = 'runtime/core/sdk/src/store/tests.rs';
const CLEANUP = 'runtime/core/src/rs/src/package/cleanup_tests.rs';
const CONTRACT = 'runtime/core/src/rs/src/contract_tests.rs';
const LFO_DEVICE = 'runtime/core/tests/lfo/lfo.device.test.ts';
const SAMPLES_RS = 'runtime/core/sdk/src/sample_tracks/tests.rs';

const RELOAD_TESTS = 'runtime/core/src/rs/src/package/reload/tests';
const RELOAD_REFUSALS = `${RELOAD_TESTS}/refusals.rs`;
const RELOAD_UNLOAD = `${RELOAD_TESTS}/unload.rs`;
const RELOAD_RELOAD = `${RELOAD_TESTS}/reload.rs`;
const PROVIDERS = 'runtime/core/tests/services/providers.device.test.ts';
const prov = (test: string): Proof => ({ file: PROVIDERS, test });
const mine = (test: string): Proof => ({ file: REFUSAL, test });
const MANAGER = 'runtime/core/src/rs/src/package/loader/tests/manager_assembly_tests.rs';
const loads = mine('the good module is ACTIVE');

/** The row of each `[load.status]` name. */
export const ROWS: Record<string, Row> = {
  E_NOT_FOUND: { fail: { file: MANAGER, test: 'a_selected_file_that_is_not_on_the_card_is_reported_and_the_rest_of_the_selection_loads' }, pass: loads },
  E_BAD_STORE: { kind: 'not-a-load', reason: 'the store header is checked before any module is selected', covered: [{ file: STORE_RS, test: 'E_BAD_STORE' }] },
  E_READ: { kind: 'open', reason: 'no boot rig makes the firmware block read fail while a module loads', covered: [{ file: STORE_RS, test: 'E_READ' }] },
  E_SHA: { fail: { file: ASSEMBLY, test: 'a_boot_records_a_file_with_a_wrong_hash_as_failed_with_e_sha' }, pass: loads },
  E_FORMAT: { fail: { file: IMAGE_RS, test: 'refuses_bad_magic_format_and_sizes' }, pass: loads },
  E_RUNTIME: { kind: 'reserved', reason: 'never returned: E_CONTRACT says a module needs another runtime' },
  E_IMPORT: { fail: { file: ASSEMBLY, test: 'an_adapter_import_resolves_only_in_an_image_with_the_adapter' }, pass: { file: ASSEMBLY, test: 'an_adapter_import_resolves_only_in_an_image_with_the_adapter' } },
  E_NOMEM: { fail: { file: CAPACITY, test: 'arena: filled to the last byte' }, pass: { file: CAPACITY, test: 'arena: filled to the last byte' } },
  E_INIT: { fail: { file: ASSEMBLY, test: 'an_init_that_returns_non_zero_is_refused_with_e_init_and_leaves_nothing' }, pass: loads },
  E_LOADED: { fail: { file: CAPACITY, test: 'files: a file of the staging buffer size' }, pass: { file: CAPACITY, test: 'files: a file of the staging buffer size' } },
  E_BUSY: { kind: 'not-a-load', reason: 'a task is still inside the module: only a teardown returns it, never the boot', covered: [{ file: CLEANUP, test: 'E_BUSY' }] },
  E_TOO_BIG: { fail: { file: CAPACITY, test: 'files: a file of the staging buffer size' }, pass: { file: CAPACITY, test: 'files: a file of the staging buffer size' } },
  E_TABLE_FULL: { kind: 'host', reason: 'the selection holds as many paths as there are module records, so the selection is refused first', covered: [{ file: CAP_RS, test: 'E_TABLE_FULL' }] },
  E_LOST: { kind: 'not-a-load', reason: 'a sample file result', covered: [{ file: SAMPLES_RS, test: 'E_LOST' }] },
  E_POOL: { kind: 'not-a-load', reason: 'a sample pool result', covered: [{ file: SAMPLES_RS, test: 'E_POOL' }] },
  E_CHANGED: { kind: 'not-a-load', reason: 'a sample file result', covered: [{ file: SAMPLES_RS, test: 'E_CHANGED' }] },
  E_EXISTS: { kind: 'not-a-load', reason: 'a dk_files_create result', covered: [{ file: 'runtime/core/tests/file-write/file-write.device.test.ts', test: 'E_EXISTS' }] },
  E_CONTEXT: { kind: 'not-a-load', reason: 'a dk_files_* call outside an I/O step', covered: [{ file: 'runtime/core/tests/file-write/file-write.device.test.ts', test: 'E_CONTEXT' }] },
  E_NO_REGISTRY: { kind: 'open', reason: 'needs an image without a registry' },
  E_REG_DEVICE: { kind: 'open', reason: 'needs a module whose register section names the other device' },
  E_TYPE_RANGE: { kind: 'host', reason: 'a patched record', covered: [{ file: CAP_RS, test: 'E_TYPE_RANGE' }] },
  E_TYPE_USED: { kind: 'host', reason: 'the registry hands out fresh types', covered: [{ file: MACHINES_BUILD, test: 'E_TYPE_USED' }] },
  E_PARAM_FULL: { kind: 'host', reason: 'collect tests fill the rows', covered: [{ file: MACHINES_BUILD, test: 'E_PARAM_FULL' }] },
  E_PAGE_FULL: { kind: 'host', reason: 'collect tests fill the rows', covered: [{ file: MACHINES_BUILD, test: 'E_PAGE_FULL' }] },
  E_TYPE_IN_USE: { kind: 'reserved', reason: 'hot reload is archived; never returned' },
  E_DECL_ID: { fail: { file: ASSEMBLY, test: 'a_machine_index_that_the_keys_table_lacks_is_refused_with_e_decl_id_and_leaves_nothing' }, pass: { file: COMMIT, test: 'the commit event and record block agree' } },
  E_DECL_USES: { fail: { file: ASSEMBLY, test: 'a_machine_declared_beyond_its_uses_is_refused_with_e_decl_uses_and_leaves_nothing' }, pass: { file: COMMIT, test: 'the commit event and record block agree' } },
  E_FX_ID_DUP: { kind: 'reserved', reason: 'the rules of the core refuse a repeated effect id (E_PLAN_KEY) at the boot and at a live load or reload (admission.rs `an_effect_id_a_live_module_has_is_refused_like_any_key`); never returned' },
  E_DECL_FULL: { kind: 'host', reason: 'more builder calls than the record limit', covered: [{ file: MACHINES_BUILD, test: 'E_DECL_FULL' }] },
  E_PLAN_KIND: { fail: { file: ASSEMBLY, test: 'a_boot_leaves_the_kind_and_the_full_resource_in_the_detail_words_of_a_refusal' }, pass: loads },
  E_PLAN_OWNER: { fail: prov('two providers of one hook site are E_PLAN_OWNER'), pass: prov('a provider installs its hook and the consumer calls it') },
  E_PLAN_KEY: { fail: { file: MANAGER, test: 'of_two_lfo_shapes_with_one_key_the_first_loads_and_the_second_is_refused_by_the_boot' }, pass: loads },
  E_PLAN_FIT: { fail: { file: ASSEMBLY, test: 'a_boot_leaves_the_kind_and_the_full_resource_in_the_detail_words_of_a_refusal' }, pass: loads },
  E_DECL_KIND: { fail: { file: LFO_DEVICE, test: 'a kind table' }, pass: { file: LFO_DEVICE, test: 'a fixture loads and unloads' } },
  E_HOOK_STOCK: { fail: prov('a port file whose check value differs is E_HOOK_STOCK and the service that needs it and its dependent are refused'), pass: prov('a provider installs its hook and the consumer calls it') },
  E_NO_DSP: { kind: 'host', reason: 'needs an image without the DSP loader', covered: [{ file: CLEANUP, test: 'E_NO_DSP' }] },
  E_DSP_FORMAT: { kind: 'host', reason: 'a malformed DSP section', covered: [{ file: CONTRACT, test: 'E_DSP_FORMAT' }] },
  E_DSP_IMPORT: { kind: 'open', reason: 'no fixture imports a DSP symbol the image lacks' },
  E_DSP_NOSPACE: { fail: { file: CAPACITY, test: 'full set' }, pass: { file: CAPACITY, test: 'full set' } },
  E_DSP_ROWS: { fail: { file: CAPACITY, test: 'full set' }, pass: { file: CAPACITY, test: 'full set' } },
  E_DSP_XFER: { kind: 'host', reason: 'a transfer that fails needs the DSP link to fail', covered: [{ file: CLEANUP, test: 'E_DSP_XFER' }] },
  E_DSP_DEVICE: { kind: 'host', reason: 'a DSP section for the other device', covered: [{ file: CONTRACT, test: 'E_DSP_DEVICE' }] },
  E_DSP_TYPE: { kind: 'host', reason: 'a voice type out of range or taken', covered: [{ file: COLLECT, test: 'E_DSP_TYPE' }] },
  E_SEL_PATH: { kind: 'not-a-load', reason: 'a SELECT result', covered: [{ file: 'runtime/core/tests/storage/storage.device.test.ts', test: 'E_SEL_PATH' }] },
  E_SEL_COUNT: {
    kind: 'not-a-load',
    reason: 'a SELECT result, and a live load that finds no free selection index',
    covered: [
      { file: CAP_RS, test: 'E_SEL_COUNT' },
      { file: 'runtime/core/abi/src/boot_path/tests.rs', test: 'a_live_load_takes_the_index_its_path_had_at_the_boot' },
    ],
  },
  E_SEL_WRITE: { kind: 'open', reason: 'the block write of the selection record fails: needs a card that refuses writes' },
  E_CARD_BUSY: { kind: 'open', reason: 'a card transfer in flight while the selection is saved' },
  E_SAFE_MODE: { fail: { file: SAFE, test: 'the safe-once flag gives safe mode' }, pass: { file: CAPACITY, test: 'files: a file of the staging buffer size' } },
  E_BOOT_ONLY: { kind: 'open', reason: 'a load call after the boot hook: no module or host command makes one' },
  E_RANGE: { kind: 'not-a-load', reason: 'a card write outside the store', covered: [{ file: 'runtime/core/tests/storage/card-safety.device.test.ts', test: 'E_RANGE' }] },
  E_CARD_RO: { kind: 'not-a-load', reason: 'the card area is not ours', covered: [{ file: 'runtime/core/tests/storage/card-safety.device.test.ts', test: 'E_CARD_RO' }] },
  E_CONTRACT: { fail: { file: IMAGE_RS, test: 'a_need_of_another_major_or_minor_or_a_higher_patch_is_refused' }, pass: loads },
  E_TABLE: { kind: 'open', reason: 'the runtime tables fail their check: needs a corrupted table' },
  E_MISSING: { fail: { file: ASSEMBLY, test: 'a_card_without_the_service_file_refuses_its_user_naming_the_service' }, pass: loads },
  E_SVC_ABI: { fail: { file: LFO_DEVICE, test: 'another ABI' }, pass: { file: LFO_DEVICE, test: 'a fixture loads and unloads' } },
  E_RELOAD_PLAYING: { kind: 'host', reason: 'a live command (reload, load or unload), not a module file, causes it; `reload.device.test.ts` drives it on both devices (the transport plays)', covered: [{ file: RELOAD_REFUSALS, test: 'playing_paused_mixed_or_unreadable' }] },
  E_RELOAD_MENUS: { kind: 'host', reason: 'a live command (reload, load or unload), not a module file, causes it', covered: [{ file: RELOAD_REFUSALS, test: 'the_refusals_come_in_the_documented_order' }] },
  E_RELOAD_ROUTED: { kind: 'host', reason: 'a live command (reload, load or unload), not a module file, causes it', covered: [{ file: RELOAD_UNLOAD, test: 'an_unload_that_is_refused_changes_nothing_and_one_of_a_module_that_is_gone_is_not_found' }] },
  E_RELOAD_RETAINED: { kind: 'host', reason: 'only a DSP loader that could not clear rows causes it', covered: [{ file: RELOAD_RELOAD, test: 'a_retained_dsp_refuses_at_the_queue' }] },
  E_RELOAD_KEYS: { kind: 'host', reason: 'a live command (reload, load or unload), not a module file, causes it', covered: [{ file: 'packages/dkneeds/src/keys.rs', test: 'a_reload_may_add_keys_but_not_drop_one' }] },
};
