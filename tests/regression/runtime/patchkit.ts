// A test that imports this file runs patchkit: its Nx merge key keeps the patchkit binary (tools/nx/device-test-keys.mjs,
// PATCHKIT_LIVE). Only runtime-plan.ts imports patchkit-exec.ts directly: its `apply --set` result is the image, plan and address map
// hashed in dist/bytes/images.sha256 (an explicit module list goes through runtime-plan-modules.ts, which imports this file).
export { patchkit, patchkitBinary } from './patchkit-exec.ts';

/** The prebuilt patchkit of the opt-in proofs (`DIGI_PATCHKIT_BIN`); undefined skips them. */
export const optInPatchkit = (): string | undefined => process.env.DIGI_PATCHKIT_BIN;
