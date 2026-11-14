// Lint check (run by `pnpm lint`): test code starts patchkit in one place, tests/regression/runtime/patchkit-exec.ts, which refuses to
// run unless the Nx task is marked live (DIGI_PATCHKIT_LIVE, set by tools/nx/module-targets.mjs from the same rule that puts the patchkit
// binary in the cache key). So outside that file and its re-export patchkit.ts:
//  - no DIGI_PATCHKIT, DIGI_PATCHKIT_BIN or dist/patchkit (the binary lookup);
//  - no patchkitBinary; patchkitCovered only in runtime-plan.ts (host tests may mock it);
//  - no execFile/spawn/exec call whose command is a patchkit variable, except in the opt-in proofs (files that import optInPatchkit,
//    which run a prebuilt binary the key hashes through DIGI_PATCHKIT_BIN in device-inputs.mjs).
import { globSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SPAWN_POINT = new Set(['tests/regression/runtime/patchkit-exec.ts', 'tests/regression/runtime/patchkit.ts']);
const files = globSync(
  [
    'tests/regression/**/*.ts',
    'tests/fixtures/**/*.ts',
    'modules/**/tests/**/*.ts',
    'packages/test-support/src/**/*.ts',
  ],
  { cwd: root, exclude: (name) => name === 'node_modules' },
).filter((file) => !file.includes('node_modules') && !SPAWN_POINT.has(file));
const problems = [];
for (const file of files) {
  const text = readFileSync(join(root, file), 'utf8');
  const lines = text.split('\n');
  const optIn = text.includes('optInPatchkit');
  lines.forEach((line, index) => {
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
  });
  if (!optIn) {
    const spawn = /\b(execFile|execFileSync|spawn|spawnSync|exec)\(\s*[\w.!()]*patchkit/gi;
    for (const match of text.matchAll(spawn))
      problems.push(`${file}: starts patchkit itself (${match[0].trim()}); call patchkit() of runtime/patchkit.ts`);
  }
}
if (problems.length) {
  console.error(`check-patchkit-spawn: ${problems.length} problem(s)\n${problems.map((p) => `  ${p}`).join('\n')}`);
  process.exit(1);
}
console.log(`check-patchkit-spawn: ok (${files.length} files)`);
