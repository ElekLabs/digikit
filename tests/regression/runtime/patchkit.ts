// A test that imports this file runs patchkit for its own sake (explicit module lists, `addr`, `synth-dkm`, a `build` check): its Nx
// merge key keeps the patchkit binary (tools/nx/module-targets.mjs, PATCHKIT_LIVE). The helpers that only prepare an image or compute an
// oracle from built bytes (runtime-plan.ts: `apply`; dkm.ts: `needs`, `fit`) import patchkit-exec.ts: what they read follows the
// bytes of dist/bytes/images.sha256 (image, plan, address map) and of the packs.
export { patchkit, patchkitBinary } from './patchkit-exec.ts';
