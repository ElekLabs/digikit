// Changed path -> acceptance areas: the one definition the area-based device gate (`mise run test`, `test:subset --base`) selects by.
// It adds no list of its own. Two existing sources say which area owns a file:
//   1. `runtime/core/owners.toml` maps the module paths of the runtime crates (`dkrt::boot`, `dkrt_lifecycle`, `dkrt_svc_lfo`, ...) to
//      record tags (`C:boot`, `S:lfo`, ...) or to `shared`. A Rust source file has the module path of its crate and its place below
//      `src/`; the longest owner prefix at a `::` boundary wins (the rule `patchkit owners` uses for the items of the object).
//   2. A service crate `services/<name>/` is `S:<name>` when a runtimeAcceptance record carries that tag.
//   3. A platform leaf `runtime/platform/<leaf>/` is `A:core/<leaf>` when a runtimeAcceptance record carries that tag (the leaf's module id is `core/<leaf>`).
//   4. A firmware declaration `ports/fw/<point>.toml` names its `users` (core modules): `core/services/<name>` is `S:<name>`, any other
//      user `A:<id>`, each when a record carries the tag. A user that names no tag (the runtime, the UI) makes the file unknown.
//   5. A `Cargo.lock` is compared with its base: when only packages without an external `source` (the repository's own crates) differ,
//      it names nothing (the effect of such a crate shows through its changed source paths); a registry or git package that was added,
//      removed or changed makes the file unknown and the message names it (`externalLockChanges`).
//   6. A crate that only dev-dependencies name (no `[dependencies]` or `[build-dependencies]` of any manifest lists it: `dkrt-test-support`)
//      is in no device artifact and names nothing: only host tests build it.
//   7. A device file (`devices/<stem>.toml`, its companions, `ports/<short>-*.toml`) names the device: its halves only.
// Principle: precise when the area is known, wide when it is not. Any other path ({ unknown }: a shared module, the SDK, a header, a
// table, patchkit, owners.toml, a crate or file outside the object) is wide when it reaches device tests: the gate then runs every device
// test `affected` selects (test-subset.mjs), never the smoke set alone.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join, matchesGlob, relative } from 'node:path';
import { parse } from 'smol-toml';
import { deviceShorts, filesOf } from './device-files.mjs';
import { isTestSource } from './test-source.mjs';

export const OWNERS_FILE = 'runtime/core/owners.toml';

// `owners.toml` -> [{ prefix, tag }] with tag `shared` for the shared set.
export const parseOwners = (text) =>
  Object.entries(parse(text)).flatMap(([tag, prefixes]) => prefixes.map((prefix) => ({ prefix, tag })));

// The owner tag of a module path: the longest prefix at a `::` boundary, or undefined.
export const ownerOf = (modulePath, owners) =>
  owners
    .filter(({ prefix }) => modulePath === prefix || modulePath.startsWith(`${prefix}::`))
    .sort((a, b) => b.prefix.length - a.prefix.length)[0]?.tag;

// The module path of a Rust file of crate `crate` rooted at `crateDir` (`src/a/b.rs` -> `crate::a::b`, `src/a/mod.rs` -> `crate::a`),
// or undefined for a file outside `src`.
export const modulePathOf = (file, crate, crateDir) => {
  const rel = relative(crateDir, file);
  if (!rel.startsWith('src/') || !rel.endsWith('.rs')) return undefined;
  const parts = rel.slice('src/'.length, -'.rs'.length).split('/');
  if (parts.at(-1) === 'mod') parts.pop();
  return [crate.replaceAll('-', '_'), ...parts].join('::');
};

// The nearest enclosing Cargo package of `file`: { name, dir }, or undefined. `read(path)` returns a file's text or undefined.
export const crateOf = (file, read = readIfThere) => {
  for (let dir = dirname(file); dir !== '.' && dir !== '/'; dir = dirname(dir)) {
    const text = read(join(dir, 'Cargo.toml'));
    const name = text === undefined ? undefined : parse(text).package?.name;
    if (name) return { name, dir };
  }
  return undefined;
};
const readIfThere = (path) => {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return undefined;
  }
};

// The packages of a Cargo.lock that come from outside the repository (they have a `source`): Map "name version" -> "source checksum".
const externalPackages = (text) =>
  new Map(
    (text === undefined ? [] : (parse(text).package ?? []))
      .filter((p) => p.source)
      .map((p) => [`${p.name} ${p.version}`, `${p.source} ${p.checksum ?? ''}`]),
  );

/** The external packages (registry or git) that differ between two lockfile texts (undefined: no file), as sorted lines naming each
 * `name version` with `added`, `removed` or `changed`. A package of the repository (no `source`) never counts: its dependency list
 * changes with every edit of a manifest, and what it builds shows in its own source paths. */
export const externalLockChanges = (baseText, headText) => {
  const before = externalPackages(baseText);
  const after = externalPackages(headText);
  const lines = [];
  for (const [key, value] of after) {
    if (!before.has(key)) lines.push(`${key} added`);
    else if (before.get(key) !== value) lines.push(`${key} changed`);
  }
  for (const key of before.keys()) if (!after.has(key)) lines.push(`${key} removed`);
  return lines.sort();
};

const PORT_FILE = /^ports\/fw\/([^/]+)\.toml$/;

// The area tags of the users of a firmware declaration file, or { unknown } naming the first user without one.
const portAreas = (file, ctx) => {
  const users = ctx.portUsers?.(file) ?? [];
  if (!users.length) return { unknown: `${file} names no users` };
  const tags = [];
  for (const user of users) {
    const service = /^core\/services\/(.+)$/.exec(user)?.[1];
    const tag = service ? `S:${service}` : `A:${user}`;
    if (!ctx.recordTags.has(tag)) return { unknown: `user ${user} of ${file} has no area (no record carries ${tag})` };
    tags.push(tag);
  }
  return { areas: [...new Set(tags)] };
};

/** `ctx`: { lockChange(file): the lines of `externalLockChanges` for a lockfile, devOnly(file): the file is in a crate that only
 * dev-dependencies name, owners, serviceTags, recordTags: Set of every tag the records carry, crateOf(file), portUsers(file), deviceOf(file) }.
 * Returns { areas: string[] }, { devices: string[] } (a device file), { none: true } (a test source: the owner's unit tests cover it) or
 * { unknown: why } (no area is known: the gate goes wide when the path reaches device tests). */
export const areasOfPath = (file, ctx) => {
  if (isTestSource(file)) return { none: true };
  let why = 'no area names it';
  if (file === 'Cargo.lock' || file.endsWith('/Cargo.lock')) {
    const changes = ctx.lockChange?.(file);
    if (changes && !changes.length) return { none: true };
    if (changes) return { unknown: `${file} changes external package(s): ${changes.join(', ')}` };
  }
  if (ctx.devOnly?.(file)) return { none: true };
  if (file.endsWith('.rs')) {
    const crate = ctx.crateOf(file);
    const mod = crate && modulePathOf(file, crate.name, crate.dir);
    const tag = mod && ownerOf(mod, ctx.owners);
    if (tag === 'shared') return { unknown: `shared runtime module ${mod}` };
    if (tag) return { areas: [tag] };
    why = mod ? `${mod} has no owner in ${OWNERS_FILE}` : 'a Rust file outside the owned modules';
  }
  const service = /^services\/([^/]+)\//.exec(file)?.[1];
  if (service && ctx.serviceTags.has(`S:${service}`)) return { areas: [`S:${service}`] };
  const leaf = /^runtime\/platform\/([^/]+)\//.exec(file)?.[1];
  if (leaf && ctx.recordTags.has(`A:core/${leaf}`)) return { areas: [`A:core/${leaf}`] };
  if (PORT_FILE.test(file)) return portAreas(file, ctx);
  const device = ctx.deviceOf?.(file);
  if (device) return { devices: [device] };
  return { unknown: why };
};

// The context of a checkout: the owner map, the tags of the records of `projects` (the Nx graph nodes), the Cargo reader, the
// users of the firmware declaration files and the device files.
// The text of `file` at `base` (undefined: not there).
const textAt = (root, base, file) => {
  const done = spawnSync('git', ['show', `${base}:${file}`], { cwd: root, encoding: 'utf8', maxBuffer: 1 << 28 });
  return done.status === 0 ? done.stdout : undefined;
};

// The directories of the crates that some manifest names only under `dev-dependencies`.
const devOnlyCrateDirs = (root) => {
  const done = spawnSync('git', ['ls-files', '*Cargo.toml'], { cwd: root, encoding: 'utf8' });
  const manifests = done.stdout.split('\n').filter(Boolean);
  const built = new Set();
  const dev = new Set();
  for (const manifest of manifests) {
    const text = readIfThere(join(root, manifest));
    if (text === undefined) continue;
    const t = parse(text);
    const tables = [t, ...Object.values(t.target ?? {})];
    for (const [kinds, into] of [
      [['dependencies', 'build-dependencies'], built],
      [['dev-dependencies'], dev],
    ])
      for (const tbl of tables)
        for (const kind of kinds)
          for (const d of Object.values(tbl[kind] ?? {}))
            if (d?.path) into.add(join(dirname(manifest), d.path).replace(/^\.\//, ''));
  }
  return [...dev].filter((dir) => !built.has(dir));
};

export const areaContext = (root, projects, base) => {
  const devOnlyDirs = devOnlyCrateDirs(root);
  const recordTags = new Set(
    projects.flatMap((p) => (p.metadata?.runtimeAcceptance ?? []).flatMap((r) => r.tags ?? [])),
  );
  const globs = deviceShorts(root).map((short) => ({ short, globs: filesOf(short, root) }));
  return {
    owners: parseOwners(readFileSync(join(root, OWNERS_FILE), 'utf8')),
    recordTags,
    serviceTags: new Set([...recordTags].filter((t) => t.startsWith('S:'))),
    crateOf: (file) => crateOf(file, (p) => readIfThere(join(root, p))),
    portUsers: (file) => {
      const text = readIfThere(join(root, file));
      if (text === undefined) return [];
      const entries = Object.values(parse(text)).flatMap((v) => (Array.isArray(v) ? v : []));
      return [...new Set(entries.flatMap((e) => (Array.isArray(e?.users) ? e.users : [])))];
    },
    lockChange: base
      ? (file) => externalLockChanges(textAt(root, base, file), readIfThere(join(root, file)))
      : undefined,
    devOnly: (file) => devOnlyDirs.some((dir) => file.startsWith(`${dir}/`)),
    deviceOf: (file) => globs.find(({ globs: g }) => g.some((glob) => matchesGlob(file, glob)))?.short,
  };
};
