import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { repoRoot } from '@digi/test-support';

const exec = promisify(execFile);
const root = repoRoot();

/** The patchkit binary: `DIGI_PATCHKIT` (the gate sets it), else dist/patchkit/patchkit (nx run patchkit:cli). */
export function patchkitBinary(): string {
  const given = process.env.DIGI_PATCHKIT;
  if (given) {
    if (!fs.existsSync(given)) throw new Error(`DIGI_PATCHKIT names a missing file: ${given}`);
    return given;
  }
  const binary = path.join(root, 'dist/patchkit/patchkit');
  if (!fs.existsSync(binary)) throw new Error(`missing ${binary}: run nx run patchkit:cli`);
  return binary;
}

type Options = { input?: string | Uint8Array; timeout?: number };

function run(args: string[], { input, timeout = 30_000 }: Options) {
  const child = exec(patchkitBinary(), ['--root', process.env.DIGI_PK_ROOT || root, ...args], {
    cwd: root,
    timeout,
    maxBuffer: 64 << 20,
    encoding: 'utf8',
  });
  if (input !== undefined) child.child.stdin?.end(input);
  return child;
}

/**
 * The one place that spawns patchkit for a test (tools/nx/check-patchkit-spawn.mjs checks it). Under Nx it throws unless the task is
 * marked live: tools/nx/module-targets.mjs sets DIGI_PATCHKIT_LIVE=1 for a task whose key holds the patchkit binary, that is, a full key
 * or an import closure that holds runtime/patchkit.ts. A test with a merge or narrow key that is not live must not run patchkit: its key
 * would not change with the binary. `DIGI_PK_ROOT` names a module variant. `input` goes to stdin.
 */
export function patchkit(args: string[], options: Options = {}) {
  if (process.env.NX_TASK_TARGET_PROJECT && !process.env.DIGI_PATCHKIT_LIVE)
    throw new Error(
      `patchkit ${args[0]}: this task's cache key does not hold the patchkit binary. Import runtime/patchkit.ts in the test (that marks it live), or read the result from dist/bytes (images.sha256, pack-verdicts.json).`,
    );
  return run(args, options);
}

/** `patchkit apply --set` for runtime-plan.ts only: its image, plan and address map are hashed in dist/bytes/images.sha256, so no flag is needed. */
export function patchkitCovered(args: string[], options: Options = {}) {
  if (args[0] !== 'apply' || !args.includes('--set')) throw new Error('patchkitCovered runs only `apply --set`');
  return run(args, options);
}
