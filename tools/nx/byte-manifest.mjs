// E8 relocation proof. See docs/TESTING.md, "Byte manifests for repository moves".
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, normalize, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { CELLS } from './device-inputs.mjs';
import { imageJobs } from './image-jobs.mjs';
import { read } from './module-manifests.mjs';
import { objectHash } from './object-hash.mjs';
import { trackedObjects } from './tracked-objects.mjs';

const DEVICES = CELLS.map(({ dev }) => dev);
const KINDS = ['image', 'plan', 'addrmap', 'pack', 'verdicts', 'generated', 'object', 'core-object-list'];
const RELOCATABLE = ['generated', 'object', 'core-object-list'];
const SHA = /^[a-f0-9]{64}$/;
const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');

const checkedPath = (path) => {
  if (
    typeof path !== 'string' ||
    !path ||
    path.includes('\\') ||
    [...path].some((char) => char < ' ') ||
    path.startsWith('/') ||
    path.split('/').some((part) => !part || part === '.' || part === '..')
  )
    throw new Error(`Invalid repository-relative path: ${JSON.stringify(path)}`);
  return path;
};

export const strictObjectHash = (bytes, file) => {
  const text = decoder.decode(bytes);
  const { source_sha256: source } = JSON.parse(text);
  // Patchkit's object encoder emits top-level properties at one-space indentation.
  const fields = [...text.matchAll(/^ "source_sha256": "([a-f0-9]{64})",?$/gm)];
  if (fields.length !== 1 || fields[0][1] !== source) throw new Error(`${file}: invalid source_sha256 field`);
  const field = fields[0];
  const start = field.index + ' "source_sha256": "'.length;
  return { source, sha256: sha(text.slice(0, start) + '0'.repeat(64) + text.slice(start + 64)) };
};

const exactKeys = (label, expected, actual) => {
  const missing = [...expected].filter((key) => !actual.has(key));
  const extra = [...actual].filter((key) => !expected.has(key));
  if (!expected.size || missing.length || extra.length)
    throw new Error(
      `${label}: ${!expected.size ? 'empty inventory; ' : ''}missing [${missing.sort()}], extra [${extra.sort()}]`,
    );
};

const addUnique = (map, key, value, label) => {
  if (map.has(key)) throw new Error(`${label}: duplicate ${key}`);
  map.set(key, value);
};

const fileBytes = (file) => {
  if (!statSync(file).isFile()) throw new Error(`${file}: expected a regular file`);
  const bytes = readFileSync(file);
  if (!bytes.length) throw new Error(`${file}: empty file`);
  return bytes;
};

export const trackedObjectHashes = (root, paths) =>
  new Map(
    [...paths].map((path) => {
      const file = join(root, path);
      const isFile = statSync(file, { throwIfNoEntry: false })?.isFile();
      return [path, isFile ? sha(readFileSync(file)) : null];
    }),
  );

export const assertTrackedObjectsUnchanged = (before, after) => {
  const refreshed = [...before]
    .filter(([path, hash]) => hash !== after.get(path))
    .map(([path]) => path)
    .sort();
  if (refreshed.length)
    throw new Error(
      `Preparation refreshed tracked objects. Review these changes before rerunning the byte manifest:\n${refreshed.join('\n')}`,
    );
};

const lines = (file) => {
  const text = decoder.decode(fileBytes(file));
  if (!text.endsWith('\n')) throw new Error(`${file}: missing final newline`);
  return text.slice(0, -1).split('\n');
};

const collectImages = (root) => {
  const file = join(root, 'dist/bytes/images.sha256');
  const values = new Map();
  for (const line of lines(file)) {
    const [cell, label, imageWord, image, planWord, plan, addrWord, addrmap, ...extra] = line.split(' ');
    if (
      imageWord !== 'image' ||
      planWord !== 'plan' ||
      addrWord !== 'addrmap' ||
      ![image, plan, addrmap].every((value) => SHA.test(value)) ||
      extra.length
    )
      throw new Error(`${file}: invalid image record ${line}`);
    addUnique(values, `${cell}/${label}`, { image, plan, addrmap, line }, file);
  }
  const jobs = new Map();
  for (const job of imageJobs(root)) {
    const { dev, ver } = job.cell;
    addUnique(jobs, `${dev}-${ver}/${job.label}`, job, file);
  }
  exactKeys(file, new Set(jobs.keys()), new Set(values.keys()));
  return new Map(
    DEVICES.map((dev) => {
      const entries = [];
      const expectedLines = new Set();
      const expectedFiles = new Set();
      const dir = join(root, 'dist/bytes/images', dev);
      for (const [path, job] of jobs) {
        if (job.cell.dev !== dev) continue;
        const { line, ...hashes } = values.get(path);
        expectedLines.add(line);
        for (const [kind, sha256] of Object.entries(hashes)) {
          const suffix = kind === 'image' ? 'syx' : `${kind}.json`;
          if (job.keep) {
            const kept = `${job.keep}.${suffix}`;
            expectedFiles.add(relative(dir, kept));
            if (sha(fileBytes(kept)) !== sha256) throw new Error(`${kept}: differs from ${file}`);
          }
          entries.push({ kind, path, sha256 });
        }
        if (!job.keep) expectedFiles.add(`set-${job.label}.info`);
      }
      exactKeys(dir, expectedFiles, new Set(readdirSync(dir)));
      const ownFile = join(root, `dist/bytes/images-${dev}.sha256`);
      const own = lines(ownFile);
      if (new Set(own).size !== own.length) throw new Error(`${ownFile}: duplicate image record`);
      exactKeys(ownFile, expectedLines, new Set(own));
      return [dev, entries];
    }),
  );
};

const collectPacks = (root, projects, entries) => {
  const inventory = new Map(DEVICES.map((dev) => [dev, new Map()]));
  const primary = new Map();
  for (const project of projects) {
    for (const target of ['pack', 'pack-aliases']) {
      // A repeated acceptance record may request the same alias from the same target.
      for (const output of new Set(project.targets?.[target]?.outputs ?? [])) {
        const match = /^\{workspaceRoot\}\/(dist\/modules\/([^/]+)\/[^/]+\.dk[ms])$/.exec(output);
        if (!match) continue;
        const [, path, dev] = match;
        const files = inventory.get(dev);
        if (!files) throw new Error(`Unknown pack device in ${path}`);
        addUnique(files, path, project.name, 'Pack producers');
        if (target === 'pack') addUnique(primary, `${dev}/${project.name}`, path, 'Primary packs');
      }
    }
  }
  const verdictFile = 'dist/bytes/pack-verdicts.json';
  const verdictBytes = fileBytes(join(root, verdictFile));
  const verdicts = JSON.parse(verdictBytes);
  const { modules } = JSON.parse(fileBytes(join(root, 'tests/regression/runtime/needs-packs.json')));
  const verdictKeys = new Set();
  for (const [dev, files] of inventory) {
    const dir = `dist/modules/${dev}`;
    const actual = readdirSync(join(root, dir), { recursive: true })
      .filter((name) => /\.dk[ms]$/.test(name))
      .map((name) => `${dir}/${name}`);
    exactKeys(dir, new Set(files.keys()), new Set(actual));
    const hashes = new Map([...files.keys()].map((path) => [path, sha(fileBytes(join(root, path)))]));
    for (const id of modules) {
      const hash = hashes.get(primary.get(`${dev}/${id}`));
      if (!hash) throw new Error(`${verdictFile}: no ${dev} pack for ${id}`);
      const key = `needs:${hash}`;
      if (
        !verdicts[key] ||
        typeof verdicts[key] !== 'object' ||
        Array.isArray(verdicts[key]) ||
        'error' in verdicts[key]
      )
        throw new Error(`${verdictFile}: no successful verdict for ${dev}/${id} (${key})`);
      verdictKeys.add(key);
    }
    entries.get(dev).push(...[...hashes].map(([path, sha256]) => ({ kind: 'pack', path, sha256 })));
  }
  exactKeys(verdictFile, verdictKeys, new Set(Object.keys(verdicts)));
  return { kind: 'verdicts', path: verdictFile, sha256: sha(verdictBytes) };
};

const collectObjects = (root, scratch, projects, tracked) => {
  const declared = new Map();
  for (const project of projects.filter((p) => p.tags?.includes('module'))) {
    const manifest = read(root, join(project.root, 'module.toml'));
    for (const object of manifest.object ?? []) {
      const path = checkedPath(normalize(join(project.root, object.path)));
      addUnique(declared, path, { project, core: manifest.module.category === 'core', object }, 'Object declarations');
    }
  }
  const objects = new Map();
  const declaredCoreObjects = new Set(
    [...declared].filter(([path, decl]) => decl.core && /\/build\/[^/]+\.dko\.json$/.test(path)).map(([path]) => path),
  );
  exactKeys(
    'Tracked core objects',
    declaredCoreObjects,
    new Set([...tracked].filter(([, { category }]) => category === 'core').map(([path]) => path)),
  );
  for (const [path, { dir }] of tracked) {
    checkedPath(path);
    const decl = declared.get(path);
    if (decl?.project.root !== dir || !decl.object.sources?.length)
      throw new Error(`${path}: expected a declared, source-backed tracked object`);
    addUnique(objects, path, { ...decl, ...strictObjectHash(fileBytes(join(root, path)), path) }, 'Tracked objects');
  }
  const coreFile = 'dist/bytes/core-objects.sha256';
  const core = new Map();
  for (const line of lines(join(root, coreFile))) {
    const match = /^(.+) ([a-f0-9]{64})$/.exec(line);
    if (!match) throw new Error(`${coreFile}: invalid object record ${line}`);
    const [, path, sha256] = match;
    addUnique(core, checkedPath(path), sha256, coreFile);
  }
  exactKeys(coreFile, declaredCoreObjects, new Set(core.keys()));
  for (const [path, hash] of core)
    if (objectHash(join(root, path)) !== hash) throw new Error(`${coreFile}: stale hash for ${path}`);
  const dirs = new Set([...objects.values()].map(({ project }) => project.root));
  execFileSync(
    'uv',
    [
      'run',
      'python',
      '-m',
      'tools.devtools.benchlock',
      'run',
      '--shared',
      '--label',
      'byte-manifest objects',
      '--',
      process.execPath,
      'tools/nx/tracked-objects.mjs',
      join(scratch, 'objects'),
      ...dirs,
    ],
    { cwd: root, stdio: 'inherit' },
  );
  return [
    ...[...objects].map(([path, { sha256, source }]) => {
      const file = join(scratch, 'objects', path);
      const built = strictObjectHash(fileBytes(file), file);
      if (built.source !== source) throw new Error(`${path}: tracked object has a stale source_sha256`);
      return { kind: 'object', path, sha256, builtSha256: built.sha256 };
    }),
    ...[...core].map(([path, sha256]) => ({ kind: 'core-object-list', path, sha256 })),
  ];
};

const manifestIndex = (manifest, device, rename = (path) => path) => {
  if (manifest?.version !== 1 || manifest.device !== device || !Array.isArray(manifest.entries))
    throw new Error(`Invalid ${device} manifest`);
  const index = new Map();
  for (const entry of manifest.entries) {
    const { kind, path, sha256, builtSha256 } = entry;
    if (
      !KINDS.includes(kind) ||
      typeof sha256 !== 'string' ||
      !SHA.test(sha256) ||
      (kind === 'object' ? typeof builtSha256 !== 'string' || !SHA.test(builtSha256) : builtSha256 !== undefined)
    )
      throw new Error(`Invalid ${device} manifest entry: ${JSON.stringify(entry)}`);
    checkedPath(path);
    const key = `${kind} ${RELOCATABLE.includes(kind) ? rename(path) : path}`;
    addUnique(index, key, { sha256, builtSha256 }, `${device} manifest`);
  }
  for (const kind of KINDS)
    if (!manifest.entries.some((entry) => entry.kind === kind))
      throw new Error(`${device} manifest: missing ${kind} entries`);
  return index;
};

export const pathMapper = (moves) => {
  if (!moves || typeof moves !== 'object' || Array.isArray(moves))
    throw new Error('Expected a JSON object of old paths to new paths');
  const dest = new Map();
  const prefixes = Object.keys(moves).sort((a, b) => b.length - a.length);
  for (const old of prefixes) addUnique(dest, checkedPath(moves[old]), checkedPath(old), 'Move destinations');
  return (path) => {
    const old = prefixes.find((prefix) => path === prefix || path.startsWith(`${prefix}/`));
    return old ? moves[old] + path.slice(old.length) : path;
  };
};

export const compareManifests = (baseline, current, device, moves = {}) => {
  const before = manifestIndex(baseline, device, pathMapper(moves));
  const after = manifestIndex(current, device);
  const problems = [];
  for (const name of [...new Set([...before.keys(), ...after.keys()])].sort()) {
    const a = before.get(name);
    const b = after.get(name);
    if (!a) problems.push(`extra ${name}`);
    else if (!b) problems.push(`missing ${name}`);
    else {
      if (a.sha256 !== b.sha256) problems.push(`changed ${name}`);
      if (a.builtSha256 !== b.builtSha256) problems.push(`changed build of ${name}`);
    }
  }
  return problems;
};

const collect = (root, scratch, tracked) => {
  const invoke = (cmd, args) => execFileSync(cmd, args, { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 ** 2 });
  const pk = (...args) => invoke(join(root, 'dist/patchkit/patchkit'), ['--root', root, ...args]);
  const graphFile = join(scratch, 'graph.json');
  invoke(process.execPath, ['tools/nx/nx.mjs', 'graph', `--file=${graphFile}`]);
  const graph = JSON.parse(fileBytes(graphFile)).graph;
  const projects = Object.values(graph.nodes).map(({ data }) => data);
  const entries = collectImages(root);
  const shared = [collectPacks(root, projects, entries)];
  const generated = JSON.parse(pk('gen', '--check', '--paths'));
  for (const path of generated)
    shared.push({ kind: 'generated', path: checkedPath(path), sha256: sha(fileBytes(join(root, path))) });
  shared.push(...collectObjects(root, scratch, projects, tracked));
  return new Map(
    [...entries].map(([device, own]) => {
      const ordered = [...own, ...shared].sort((a, b) => {
        const left = `${a.kind} ${a.path}`;
        const right = `${b.kind} ${b.path}`;
        return left < right ? -1 : Number(left > right);
      });
      const manifest = { version: 1, device, entries: ordered };
      manifestIndex(manifest, device);
      return [device, manifest];
    }),
  );
};

const main = () => {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: { baseline: { type: 'string' }, 'move-map': { type: 'string' } },
  });
  const [command] = positionals;
  if (
    positionals.length !== 1 ||
    !['write', 'compare'].includes(command) ||
    (values['move-map'] && command !== 'compare')
  )
    throw new Error('usage: node tools/nx/byte-manifest.mjs write|compare [--baseline DIR] [--move-map FILE]');
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const baseline = resolve(values.baseline ?? join(root, 'out/e8'));
  const filenames = new Map(DEVICES.map((dev) => [dev, `manifest-${dev}.json`]));
  const baselineFiles = new Map();
  const moves = values['move-map'] ? JSON.parse(fileBytes(resolve(values['move-map']))) : {};
  if (command === 'compare') {
    const actual = readdirSync(baseline).filter((name) => /^manifest-.*\.json$/.test(name));
    exactKeys(baseline, new Set(filenames.values()), new Set(actual));
    for (const [dev, file] of filenames) {
      const manifest = JSON.parse(fileBytes(join(baseline, file)));
      manifestIndex(manifest, dev, pathMapper(moves));
      baselineFiles.set(dev, manifest);
    }
  }
  const tracked = trackedObjects(root);
  // Use raw hashes within this checkout so even a source_sha256 refresh fails preparation.
  const objectHashes = trackedObjectHashes(root, tracked.keys());
  // Service packs can read tracked core objects. Finish their producers before starting any pack or image task.
  // Nx owns both preparation locks. The scratch build takes its own lock after preparation exits.
  for (const targets of ['build-objects', 'pack,pack-aliases,device-images,device-bytes,pack-verdicts']) {
    execFileSync(process.execPath, ['tools/nx/nx.mjs', 'run-many', '-t', targets], { cwd: root, stdio: 'inherit' });
    assertTrackedObjectsUnchanged(objectHashes, trackedObjectHashes(root, tracked.keys()));
  }
  const scratch = mkdtempSync(join(tmpdir(), 'digi-byte-manifest-'));
  try {
    const current = collect(root, scratch, tracked);
    if (command === 'write') {
      mkdirSync(baseline, { recursive: true });
      for (const [dev, manifest] of current) {
        const file = join(baseline, filenames.get(dev));
        writeFileSync(file, `${JSON.stringify(manifest, null, 2)}\n`);
        console.log(`Wrote ${file} (${manifest.entries.length} entries)`);
      }
      return;
    }
    const problems = [];
    for (const [dev, manifest] of current)
      problems.push(
        ...compareManifests(baselineFiles.get(dev), manifest, dev, moves).map((message) => `${dev}: ${message}`),
      );
    if (problems.length) throw new Error(problems.join('\n'));
    console.log(`Byte manifests match for ${DEVICES.join(' and ')}`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
};

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main();
  } catch (error) {
    console.error(`byte-manifest: ${error.message}`);
    process.exitCode = 1;
  }
}
