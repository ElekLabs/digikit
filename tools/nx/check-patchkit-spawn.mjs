// Lint check (run by `pnpm lint`): test code starts patchkit in one place, tests/regression/runtime/patchkit-exec.ts, which refuses to
// run unless the Nx task is marked live (DIGI_PATCHKIT_LIVE, set by tools/nx/module-targets.mjs from the same rule that puts the patchkit
// binary in the cache key). Scanned: the .ts and .mjs files of the test dirs and of the helper packages the device tests import. Outside
// patchkit-exec.ts and its re-export patchkit.ts:
//  - no DIGI_PATCHKIT, DIGI_PATCHKIT_BIN or dist/patchkit (the binary lookup);
//  - no patchkitBinary; patchkitCovered only in runtime-plan.ts (host tests may mock it);
//  - no execFile/spawn/exec call whose command mentions patchkit, except a command that is `optInPatchkit()` or a variable assigned
//    from it in the same file (the opt-in proofs run a prebuilt binary that device-inputs.mjs hashes through DIGI_PATCHKIT_BIN);
//  - no Python tool that runs the binary or the harness: tools.patchkit.patchkit, pk_harness, pk_dkrt*, pk_emud*, pk_device, pk_mem,
//    pk_proof* (the pure analyzers pk_digikit_* and hook_ab that analysisJson runs read artifacts only).
import { globSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SPAWN_POINT = new Set(['tests/regression/runtime/patchkit-exec.ts', 'tests/regression/runtime/patchkit.ts']);
const DIRS = [
  'tests/**',
  'modules/**/tests/**',
  'packages/test-support/src/**',
  'packages/dk-session/src/**',
  'packages/control-client/src/**',
  'packages/dkbs/src/**',
  'apps/web/src/modules/**',
];
const files = globSync(
  DIRS.flatMap((dir) => [`${dir}/*.ts`, `${dir}/*.mjs`]),
  { cwd: root, exclude: (name) => name === 'node_modules' || name === 'target' },
).filter((file) => !file.includes('node_modules') && !SPAWN_POINT.has(file));
const PY_RUNNERS = /tools[./]patchkit[./](patchkit|pk_harness|pk_dkrt\w*|pk_emud\w*|pk_device|pk_mem|pk_proof\w*)\b/;
const SPAWN = /\b(execFile|execFileSync|spawn|spawnSync|exec)\(\s*([^,]*)/g;
const problems = [];
for (const file of files) {
  const text = readFileSync(join(root, file), 'utf8');
  const optIn = new Set([...text.matchAll(/\b(?:const|let)\s+(\w+)\s*=\s*optInPatchkit\(\)/g)].map((m) => m[1]));
  text.split('\n').forEach((line, index) => {
    const at = `${file}:${index + 1}`;
    if (/^\s*(\/\/|\*|\/\*)/.test(line)) return;
    if (/DIGI_PATCHKIT\b|DIGI_PATCHKIT_BIN|dist\/patchkit/.test(line))
      problems.push(`${at}: use runtime/patchkit.ts, not the binary lookup`);
    if (/patchkitBinary/.test(line))
      problems.push(`${at}: patchkitBinary is for patchkit-exec.ts only; call patchkit()`);
    if (
      /patchkitCovered/.test(line) &&
      file !== 'tests/regression/runtime/runtime-plan.ts' &&
      !file.startsWith('tests/regression/host/')
    )
      problems.push(`${at}: patchkitCovered is for runtime-plan.ts only`);
    if (PY_RUNNERS.test(line)) problems.push(`${at}: a Python tool that runs the patchkit binary or the harness`);
  });
  // a command may follow on the next line: `exec(\n  patchkit!,`
  for (const match of text.matchAll(SPAWN)) {
    const command = match[2].trim().replace(/!$/, '');
    if (!/patchkit/i.test(command)) continue;
    if (command === 'optInPatchkit()' || optIn.has(command)) continue;
    const line = text.slice(0, match.index).split('\n').length;
    problems.push(
      `${file}:${line}: starts patchkit itself (${match[0].trim()}); call patchkit() of runtime/patchkit.ts`,
    );
  }
}
if (problems.length) {
  console.error(`check-patchkit-spawn: ${problems.length} problem(s)\n${problems.map((p) => `  ${p}`).join('\n')}`);
  process.exit(1);
}
console.log(`check-patchkit-spawn: ok (${files.length} files)`);
