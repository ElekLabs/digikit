// Which changed paths are inputs of a device or conformance test task: Nx's own answer (`nx affected --files=...` over the test-device
// and test-conformance targets, so the merge and narrow keys of tools/nx/device-test-keys.mjs, the patched images' sources and every
// dependency output count), never a list of ours. The area gate (tools/nx/test-subset.mjs) goes wide for such a path that names no area.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const CHUNK = 200;

// The device-half and conformance tasks that Nx `affected` selects for the changed `files` (task ids).
export const nxAffectedByFiles = (root, files) => {
  const dir = mkdtempSync(join(tmpdir(), 'device-reach-'));
  const graph = join(dir, 'affected.json');
  try {
    const done = spawnSync(
      'node',
      [
        resolve(root, 'tools/nx/nx.mjs'),
        'affected',
        `--files=${files.join(',')}`,
        '-t',
        'test-device,test-conformance',
        `--graph=${graph}`,
      ],
      { cwd: root, encoding: 'utf8', maxBuffer: 1 << 28 },
    );
    if (done.status !== 0) throw new Error(`nx affected --files failed:\n${(done.stderr || done.stdout).slice(-800)}`);
    // Nx writes the graph also when nothing is affected (an empty task map): a missing or unreadable graph is a failure, and the
    // caller must not narrow on it.
    let text;
    try {
      text = readFileSync(graph, 'utf8');
    } catch (error) {
      throw new Error(`nx affected --files wrote no graph: ${error.message}`);
    }
    return Object.keys(JSON.parse(text).tasks?.tasks ?? {});
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const isTestTask = (id) => /:test-device--(?:.+--)?[a-z0-9]+$/.test(id) || id.endsWith(':test-conformance');

// The files among `files` that are inputs of some device or conformance task. `affected(files)`: the task ids Nx selects for them.
// A group no task reads is dropped in one call; a group some task reads is split until single files remain. With `first`, the walk
// stops at the first such file (the area gate needs one to go wide).
export const reachingDeviceTests = (
  root,
  files,
  affected = (f) => nxAffectedByFiles(root, f),
  { first = false } = {},
) => {
  const hit = [];
  const walk = (group) => {
    if (first && hit.length) return;
    if (!group.length || !affected(group).some(isTestTask)) return;
    if (group.length === 1) return void hit.push(group[0]);
    const mid = group.length >> 1;
    walk(group.slice(0, mid));
    walk(group.slice(mid));
  };
  for (let i = 0; i < files.length && !(first && hit.length); i += CHUNK) walk(files.slice(i, i + CHUNK));
  return hit;
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(fileURLToPath(import.meta.url), '../../..');
  const files = process.argv.slice(2);
  const hit = new Set(reachingDeviceTests(root, files));
  for (const f of files) console.log(`${hit.has(f) ? 'device-test input' : 'no device input'}  ${f}`);
}
