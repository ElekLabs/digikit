// Changed path -> acceptance areas: the one definition the area-based device gate (`mise run test`, `test:subset --base`) selects by.
// It adds no list of its own. Two existing sources say which area owns a file:
//   1. `runtime/core/owners.toml` maps the module paths of the runtime crates (`dkrt::boot`, `dkrt_lifecycle`, `dkrt_svc_lfo`, ...) to
//      record tags (`C:boot`, `S:lfo`, ...) or to `shared`. A Rust source file has the module path of its crate and its place below
//      `src/`; the longest owner prefix at a `::` boundary wins (the rule `patchkit owners` uses for the items of the object).
//   2. A service crate `services/<name>/` is `S:<name>` when a runtimeAcceptance record carries that tag.
// A file in neither (a shared module, a header, a table, a tool) has no area: the gate runs the fixed smoke set for it and the tests of the
// owner that the other rules find (records whose own project or modules changed, tests that import the file).
import { readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { parse } from 'smol-toml';
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

/** `ctx`: { owners, serviceTags: Set of the `S:` tags the records carry, crateOf(file) }.
 * Returns { areas: string[] } (a file of an owner area or a service), { shared: true } (an owner of every area) or { none: true }.
 * Test sources have no area: the owner's unit tests cover them. */
export const areasOfPath = (file, ctx) => {
  if (isTestSource(file)) return { none: true };
  if (file.endsWith('.rs')) {
    const crate = ctx.crateOf(file);
    const mod = crate && modulePathOf(file, crate.name, crate.dir);
    const tag = mod && ownerOf(mod, ctx.owners);
    if (tag === 'shared') return { shared: true };
    if (tag) return { areas: [tag] };
  }
  const service = /^services\/([^/]+)\//.exec(file)?.[1];
  if (service && ctx.serviceTags.has(`S:${service}`)) return { areas: [`S:${service}`] };
  return { none: true };
};

// The context of a checkout: the owner map, the `S:` tags of the records of `projects` (the Nx graph nodes) and the Cargo reader.
export const areaContext = (root, projects) => ({
  owners: parseOwners(readFileSync(join(root, OWNERS_FILE), 'utf8')),
  serviceTags: new Set(
    projects
      .flatMap((p) => (p.metadata?.runtimeAcceptance ?? []).flatMap((r) => r.tags ?? []))
      .filter((t) => t.startsWith('S:')),
  ),
  crateOf: (file) => {
    const found = crateOf(file, (p) => readIfThere(join(root, p)));
    return found;
  },
});
