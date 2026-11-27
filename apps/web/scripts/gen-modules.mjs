// Build step of the patch gallery: writes public/modules/bundle.json (what patchkit.wasm reads: ports + modules as
// texts) from the committed ports and module roots. Never reads out/modules (generated oracles hold firmware
// words). The bundle equals `tools/patchkit/patchkit.py bundle` (the node test compares them).
//
// The objects of content modules (every module but category "core") are build output, not committed: `patchkit build` (the Rust CLI, run through cargo) makes them
// first (it needs the m68k-elf toolchain; nothing is built when they are current) and they are read from the cache out/cache/dko/<id>/<src12>/ (`patchkit build MODDIR... --paths` names the files).
// It also writes what the Transfer page ships (see shipModules): per device the prebuilt .dkm of each run-time content module
// (public/modules/<dev>/<leaf>.dkm, the bytes the workbench app's `lib_build` writes to out/modules/<dev>/), public/modules/index.json and the sources of the
// modules (public/modules/src/<id>/: module.toml, README.md, src/**; nothing of build/ or out/).
//
// The objects of `core/*` modules are committed in the module and are only read here, never rebuilt. The browser never builds: patchkit.wasm packs from the bundle.
//
//   node scripts/gen-modules.mjs [--out DIR] [--workbench-preview [--real-packs]]
import { readFileSync, readdirSync, writeFileSync, mkdirSync, existsSync, statSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, dirname, resolve, sep, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { parse } from 'smol-toml';
import { isShipped, shippedDevices, SHIPPED_KINDS } from '../src/data/shipped.mjs';
import { SITE_DEVICES } from '../src/data/site-devices.mjs';
import { loadDevices } from '@digi/devices/node';

const here = dirname(fileURLToPath(import.meta.url));
// The devices the site ships modules for: the runtime devices that the site offers (src/data/site-devices.mjs). A module whose targets are all outside the list is not shipped or listed.
const deviceIds = loadDevices().runtimeIds.filter((d) => SITE_DEVICES.includes(d));
export const repoRoot = resolve(here, '../../..');

const walk = (dir, name, out = []) => {
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, e.name);
    if (e.isDirectory()) walk(full, name, out);
    else if (e.name === name) out.push(full);
  }
  return out;
};
// Python's sorted(Path) compares path parts, not the joined string.
const byParts = (a, b) => {
  const x = a.split(sep),
    y = b.split(sep);
  for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return x.length - y.length;
};

const GALLERY = 'build/gen/gallery.json';

/**
 * `patchkit build DIR...` over the given content-module directories: builds their stale objects into the cache.
 * Core modules are never passed: their objects are committed and read as they are (`patchkit:objects-check` finds a stale one).
 */
export function buildObjects(dirs, root = repoRoot) {
  if (!dirs.length) return;
  try {
    patchkit(['build', ...dirs], { root });
  } catch (e) {
    throw new Error(
      `patchkit build failed (the gallery needs the module objects; toolchain: uv run python -m tools.patchkit.pk_toolchains m68k):\n${e.message}`,
    );
  }
}

/** Runs the Rust patchkit CLI from the repository `root`; returns stdout, throws an Error with the stderr tail on failure. */
function patchkit(args, { root = repoRoot } = {}) {
  const env = { ...process.env };
  delete env.PATCHKIT_MODULES; // only the committed roots go into the bundle
  try {
    return execFileSync(
      'cargo',
      ['run', '-q', '--release', '--offline', '-p', 'patchkit', '--', '--root', root, ...args],
      {
        cwd: root,
        env,
        encoding: 'utf8',
        stdio: 'pipe',
        maxBuffer: 256 * 1024 * 1024,
      },
    );
  } catch (e) {
    const err = new Error(
      String(e.stderr ?? e.message ?? e)
        .trim()
        .split('\n')
        .slice(-40)
        .join('\n'),
    );
    err.stdout = String(e.stdout ?? '');
    throw err;
  }
}

/** `patchkit build DIR... --paths`: per directory, the stored file of each object with sources, for the sources now (the cache entry is keyed by them). */
function cachedPaths(root, dirs) {
  if (!dirs.length) return {};
  return JSON.parse(patchkit(['build', ...dirs, '--paths'], { root }));
}

/** The text of an object: in the module for `core/*`, else in the cache (`stored`: the paths of `cachedPaths`). */
const objectText = (dir, d, o, stored) =>
  readFileSync(
    d.module.category !== 'core' && (o.sources ?? []).length > 0 ? stored[o.path] : join(dir, o.path),
    'utf8',
  );

/** The device ids that a module is for: the prefixes of its `ports` and `[targets]`; empty: every device. */
const moduleDevices = (d) => [...(d.module.ports ?? []), ...Object.keys(d.targets ?? {})].map((p) => p.split('-')[0]);

/**
 * `{bundle}` from the committed tree under `root` plus the built objects of the content modules. `offered`: only the ports of these devices go in, and the modules
 * that are for other devices only (the site passes its list: the browser patches only what the site offers; the device files stay whole, they identify a user's firmware).
 * Omitted: everything (the Python bundle's content, which a test compares).
 * @param {string} [root]
 * @param {readonly string[]} [offered]
 */
export function buildModuleData(root = repoRoot, offered = undefined, { firmwareOnly = false } = {}) {
  const ports = readdirSync(join(root, 'ports'))
    .filter((f) => f.endsWith('.toml') && (!offered || offered.includes(f.split('-')[0])))
    .sort()
    .map((f) => readFileSync(join(root, 'ports', f), 'utf8'));
  // the module roots in the order of tools/patchkit/modroots.py REPO_ROOTS; the tests use the fixtures
  // The published site leaves out the test fixtures: only the site's tests use them, and some need the Rust toolchain for ColdFire.
  const roots = ['modules', 'runtime', 'services'];
  if (!firmwareOnly && !process.env.DIGI_SITE_NO_FIXTURES) roots.push('tests/fixtures/modules');
  const manifests = roots.flatMap((r) => walk(join(root, r), 'module.toml').sort(byParts));
  const parsed = manifests
    .map((file) => {
      const text = readFileSync(file, 'utf8');
      return { dir: dirname(file), text, d: parse(text) };
    })
    .filter(({ d }) => {
      if (firmwareOnly && d.module.category !== 'core') return false;
      const mine = moduleDevices(d);
      return !offered || mine.length === 0 || mine.some((x) => offered.includes(x));
    });
  const built = (d) => d.module.category !== 'core' && (d.object ?? []).some((o) => (o.sources ?? []).length > 0);
  const builtDirs = parsed.filter((m) => built(m.d)).map((m) => m.dir);
  buildObjects(builtDirs, root);
  const storedByDir = cachedPaths(root, builtDirs);
  const modules = [];
  for (const { dir, text, d } of parsed) {
    const files = {};
    const stored = built(d) ? (storedByDir[dir] ?? {}) : {};
    for (const o of d.object ?? []) files[o.path] = objectText(dir, d, o, stored);
    for (const r of d.registry ?? [])
      if (r.index_table) files[r.index_table] = readFileSync(join(dir, r.index_table), 'utf8');
    // `patchkit gen`: machines and parameters, for the page
    if (existsSync(join(dir, GALLERY))) files[GALLERY] = readFileSync(join(dir, GALLERY), 'utf8');
    modules.push({ manifest: text, files });
  }
  // the device files with a [modules] section: the knob models the typed module sections are lowered with
  const devices = readdirSync(join(root, 'devices'))
    .filter((f) => f.endsWith('.toml'))
    .sort()
    .map((f) => readFileSync(join(root, 'devices', f), 'utf8'))
    .filter((t) => 'modules' in parse(t));
  return { bundle: { ports, modules, devices } };
}

const SOURCE_MAX = 256 * 1024;

/** The files of a module that the site shows: module.toml, README.md and src/** (text only), relative to the module, sorted. */
function sourceFiles(dir) {
  const out = ['module.toml', 'README.md'].filter((f) => existsSync(join(dir, f)));
  out.push(
    ...readdirSync(dir, { withFileTypes: true })
      .filter((e) => e.isFile() && e.name.startsWith('LICENSE'))
      .map((e) => e.name)
      .sort(),
  );
  const walkSrc = (d) => {
    for (const e of readdirSync(join(dir, d), { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const rel = `${d}/${e.name}`;
      if (e.isDirectory()) walkSrc(rel);
      else if (e.isFile()) out.push(rel);
    }
  };
  if (existsSync(join(dir, 'src'))) walkSrc('src');
  return out;
}

const sha256 = (b) => createHash('sha256').update(b).digest('hex');

/** `patchkit pack-all` of the ids for one device: `{id: bytes}` of the packed ones; throws when any fails. */
function packAll(root, ids, device, to) {
  mkdirSync(to, { recursive: true });
  let stdout;
  let failure = '';
  try {
    stdout = patchkit(['pack-all', '--device', device, '--out', to, ...ids], { root });
  } catch (e) {
    stdout = e.stdout ?? '';
    failure = e.message;
  }
  const packed = {};
  const failed = [];
  for (const line of stdout.split('\n').filter((l) => l.trim().startsWith('{'))) {
    const r = JSON.parse(line);
    if (r.error !== undefined) failed.push(`${r.id}: ${r.error}`);
    else if (r.file !== undefined) packed[r.id] = readFileSync(resolve(root, r.file));
  }
  if (failed.length) throw new Error(`patchkit pack failed for ${failed.join('; ')} on ${device}`);
  if (failure) throw new Error(`patchkit pack failed on ${device}: ${failure}`);
  return packed;
}

/** Write the shipped modules under `out`: <dev>/<leaf>.dkm, index.json and src/<id>/**. Returns the index. */
export function shipModules(out, root = repoRoot, extraModules = new Set(), { simulated = false } = {}) {
  const scratch = join(root, 'out', 'cache', 'site-pack');
  for (const sub of [...loadDevices().runtimeIds, 'src']) rmSync(join(out, sub), { recursive: true, force: true });
  const modules = [];
  const manifests = SHIPPED_KINDS.flatMap((r) => walk(join(root, 'modules', r), 'module.toml').sort(byParts));
  const shipped = [];
  const idsByDevice = {};
  for (const file of manifests) {
    const dir = dirname(file);
    const d = parse(readFileSync(file, 'utf8'));
    if (!extraModules.has(d.module.id) && !isShipped(d, relative(root, dir).split(sep).join('/'))) continue;
    shipped.push({ dir, d });
    for (const dev of shippedDevices(d, deviceIds)) (idsByDevice[dev] ??= []).push(d.module.id);
  }
  const packed = {};
  if (!simulated)
    for (const [dev, ids] of Object.entries(idsByDevice)) packed[dev] = packAll(root, ids, dev, join(scratch, dev));
  for (const { dir, d } of shipped) {
    const m = d.module;
    const leaf = m.id.split('/').pop();
    const devices = {};
    for (const dev of shippedDevices(d, deviceIds)) {
      if (simulated) {
        const group = d.machine?.length ? 'machine' : d.fx?.length ? 'fx' : 'tool';
        const keys = (d[group] ?? []).map((entry) => entry.id);
        devices[dev] = {
          file: `${dev}/${leaf}.dkm`,
          size: 4096,
          fixture: { group, keys, uses: d.uses?.[dev] ?? {}, extraServices: d.workbench_preview?.extra_services ?? [] },
        };
        continue;
      }
      const bytes = packed[dev][m.id];
      if (!bytes) continue; // skipped by the packer (patch-only)
      mkdirSync(join(out, dev), { recursive: true });
      writeFileSync(join(out, dev, `${leaf}.dkm`), bytes);
      devices[dev] = { file: `${dev}/${leaf}.dkm`, size: bytes.length, sha256: sha256(bytes) };
    }
    if (!Object.keys(devices).length) continue;
    const files = simulated ? [] : sourceFiles(dir);
    for (const f of files) {
      const bytes = readFileSync(join(dir, f));
      if (bytes.length > SOURCE_MAX) throw new Error(`${m.id}/${f} is too large to show (${bytes.length} bytes)`);
      const to = join(out, 'src', m.id, f);
      mkdirSync(dirname(to), { recursive: true });
      writeFileSync(to, bytes);
    }
    modules.push({
      id: m.id,
      name: m.name,
      summary: m.summary,
      kind: m.category,
      version: m.version,
      devices,
      source: files,
    });
  }
  // the .dks services as built (packed here, no firmware in them) and the port files of the firmware versions the repository has a port for: Transfer uploads a service
  // as it is, with the port file whose FW item equals the device's fw_id. The build fails when either is missing: a site without them installs no service.
  const services = {};
  const serviceManifests = walk(join(root, 'services'), 'module.toml')
    .sort(byParts)
    .map((file) => parse(readFileSync(file, 'utf8')))
    .filter((d) => d.module.role === 'service');
  for (const dev of deviceIds) {
    const ids = serviceManifests
      .filter((d) => Object.keys(d.targets ?? {}).some((t) => t.startsWith(`${dev}-`)))
      .map((d) => d.module.id);
    if (!ids.length) continue;
    const packed = packAll(root, ids, dev, join(scratch, `${dev}-services`));
    for (const id of ids) {
      const bytes = packed[id];
      if (!bytes)
        throw new Error(`the service ${id} did not pack for ${dev}: the site would ship no ${id.split('/').pop()}.dks`);
      const name = `${id.split('/').pop()}.dks`;
      mkdirSync(join(out, dev), { recursive: true });
      writeFileSync(join(out, dev, name), bytes);
      (services[dev] ??= []).push({ file: `${dev}/${name}`, size: bytes.length, sha256: sha256(bytes) });
    }
  }
  const ports = {};
  const portsDir = join(scratch, 'ports');
  rmSync(portsDir, { recursive: true, force: true });
  mkdirSync(portsDir, { recursive: true });
  patchkit(['port', 'build', '--all', '--out', portsDir], { root });
  for (const name of readdirSync(portsDir)
    .filter((f) => f.endsWith('.dkport'))
    .sort()) {
    const dev = deviceIds.find((d) => name.startsWith(`${d}-`));
    if (!dev) continue;
    const bytes = readFileSync(join(portsDir, name));
    mkdirSync(join(out, dev), { recursive: true });
    writeFileSync(join(out, dev, name), bytes);
    (ports[dev] ??= []).push({ file: `${dev}/${name}`, size: bytes.length, sha256: sha256(bytes) });
  }
  for (const dev of Object.keys(services))
    if (!ports[dev]?.length)
      throw new Error(`no port file was built for ${dev}: the site would ship services that no firmware can use`);
  const index = { modules, services, ports };
  writeFileSync(join(out, 'index.json'), JSON.stringify(index, null, 1));
  return index;
}

const MODULE_FILES_TS = join(
  here,
  '..',
  '..',
  '..',
  'packages',
  'workbench-core',
  'src',
  'transfer',
  'module-files.generated.ts',
);

/**
 * The text of packages/workbench-core/src/transfer/module-files.generated.ts: module id to the `[files]` table of its module.toml (the files it owns under
 * `data/<module id>/`). Committed, because the desktop build does not run this script; `--check` fails when the file is stale.
 */
export function moduleFilesSource(root = repoRoot) {
  const entries = [];
  const manifests = SHIPPED_KINDS.flatMap((r) => walk(join(root, 'modules', r), 'module.toml').sort(byParts));
  for (const file of manifests) {
    const d = parse(readFileSync(file, 'utf8'));
    if (!d.files) continue;
    const spec = { name: d.module.name, label: d.files.label, ext: d.files.ext, preview: d.files.preview };
    entries.push(`  ${JSON.stringify(d.module.id)}: ${JSON.stringify(spec)},`);
  }
  return [
    '// Generated by apps/web/scripts/gen-modules.mjs from the [files] tables of module.toml. Do not edit.',
    "import type { ModuleFilesSpec } from './env';",
    '',
    'export const MODULE_FILES: Record<string, ModuleFilesSpec> = {',
    ...entries,
    '};',
    '',
  ].join('\n');
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url) &&
  process.argv.includes('--check')
) {
  const stale = !existsSync(MODULE_FILES_TS) || readFileSync(MODULE_FILES_TS, 'utf8') !== moduleFilesSource();
  if (stale) {
    console.error(
      'packages/workbench-core/src/transfer/module-files.generated.ts is stale: run node apps/web/scripts/gen-modules.mjs',
    );
    process.exit(1);
  }
  console.log('module-files.generated.ts is current');
} else if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const preview = process.argv.includes('--workbench-preview');
  if (!preview) writeFileSync(MODULE_FILES_TS, moduleFilesSource());
  const i = process.argv.indexOf('--out');
  const out = preview
    ? join(repoRoot, 'dist', 'workbench-preview')
    : i > 0
      ? resolve(process.argv[i + 1])
      : join(here, '..', 'public', 'modules');
  const simulated = preview && !process.argv.includes('--real-packs');
  const { bundle } = simulated
    ? buildModuleData(repoRoot, SITE_DEVICES, { firmwareOnly: true })
    : buildModuleData(repoRoot, SITE_DEVICES);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'bundle.json'), JSON.stringify(bundle));
  const devices = loadDevices();
  // The desktop installs these after a boot, including tools excluded from the public site.
  const defaults = preview ? new Set(deviceIds.flatMap((id) => devices.get(id).defaultModules)) : undefined;
  const index = shipModules(out, repoRoot, defaults, { simulated });
  console.log(`shipped: ${index.modules.map((m) => `${m.id} (${Object.keys(m.devices).join(',')})`).join(', ')}`);
  console.log(
    `modules: ${bundle.modules.length} modules, ${bundle.ports.length} ports, bundle ${statSync(join(out, 'bundle.json')).size} bytes`,
  );
}
