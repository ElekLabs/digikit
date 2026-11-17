import { execFile, execFileSync } from 'node:child_process';
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

/** An Nx task: tools/nx/device-tests.mjs sets DIGI_PATCHKIT_LIVE to '1' or '' for every device task, so a value exported in a shell never leaks in. */
const underNx = () =>
  process.env.DIGI_PATCHKIT_LIVE !== undefined || Object.keys(process.env).some((name) => name.startsWith('NX_TASK_'));

/**
 * The one place that spawns patchkit for a test (tools/nx/check-patchkit-spawn.mjs checks it). Under Nx it throws unless the task is
 * marked live: tools/nx/device-tests.mjs sets DIGI_PATCHKIT_LIVE=1 for a task whose key holds the patchkit binary, that is, a full key
 * or an import closure that holds runtime/patchkit.ts. A test with a merge or narrow key that is not live must not run patchkit: its key
 * would not change with the binary. `DIGI_PK_ROOT` names a module variant. `input` goes to stdin.
 */
export function patchkit(args: string[], options: Options = {}) {
  if (underNx() && process.env.DIGI_PATCHKIT_LIVE !== '1')
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

/** A service the card needs: its name (`services/<name>.dks`) and the module that provides it. */
export type CardService = { service: string; moduleId: string };

/**
 * `patchkit services` without a bound file: which card services the packed `.dkm` files need (`files`) or which module provides the service `name`s, from the NEEDS
 * of the files and the manifests (the one rule, packages/patchkit/src/services.rs). This is no live run: the answer is a function of the packed files and the manifests,
 * which are already in the key of every test that installs a module, so the binary stays out of the key. Binding (`--bind-dir`) is refused here: the Nx target
 * `<service id>:bind` makes the bound files.
 */
export function patchkitServices(
  device: string,
  { files = [], names = [] }: { files?: string[]; names?: string[] },
): CardService[] {
  const args = ['services', '--device', device, ...names.flatMap((n) => ['--service', n]), ...files];
  const out = execFileSync(patchkitBinary(), ['--root', process.env.DIGI_PK_ROOT || root, ...args], {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 1 << 20,
  });
  return out
    .split('\n')
    .filter(Boolean)
    .map((line) => {
      const v = JSON.parse(line) as { service: string; module_id: string };
      return { service: v.service, moduleId: v.module_id };
    });
}
