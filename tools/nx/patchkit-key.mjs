// Writes dist/bytes/patchkit-test-<device>.key for every device file: the cache key of a device test that runs patchkit live. A live test
// follows what patchkit does, not the bytes of the binary: the key is one sha256 line per repository file that the patchkit build read (the
// dependency file cargo wrote, `dist/cargo/release/patchkit.d`), without the files of the other devices and without test-only Rust, plus the
// manifests, Cargo.lock, `.cargo/`, the files of the device itself, `rustc -vV` and the profile and rustflags environment. Paths are
// repository-relative, so every lane and the remote cache get the same key. Same shape as `emud_key.test_key_of` (the daemon test key).
// Full keys (test-device-all, conformance) keep the binary. Run by `patchkit:cli` after the build (tools/nx/cargo.mjs); it fails when the
// dependency file is missing or lists a file that is gone: build patchkit again.
import { spawnSync } from 'node:child_process';
import {
  existsSync,
  globSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import { sha256File } from './digest.mjs';
import { walkFiles } from './tree.mjs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { releaseProfileEnv } from './cargo-profile-env.mjs';
import { isTestSource } from './test-source.mjs';
import { deviceShorts, filesOf, otherFiles } from './device-files.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const DEPFILE = 'dist/cargo/release/patchkit.d';
export const KEY_DIR = 'dist/bytes';
export const keyFile = (device) => `${KEY_DIR}/patchkit-test-${device}.key`;

/** The dependencies of a make-style .d file (`target: dep dep`, `\ ` escapes a space, `\` + newline continues); comment lines are skipped. */
export const parseDepfile = (text) => {
  const deps = [];
  for (const line of text.replace(/\\\r?\n/g, ' ').split(/\r?\n/)) {
    if (line.startsWith('#')) continue;
    let cut = -1;
    for (let i = 0; i < line.length; i++) {
      const separator = line[i] === ':' && (i + 1 === line.length || line[i + 1] === ' ' || line[i + 1] === '\t');
      if (separator && (i === 0 || line[i - 1] !== '\\')) {
        cut = i;
        break;
      }
    }
    if (cut < 0) continue;
    let token = '';
    for (let i = cut + 1; i < line.length; i++) {
      const ch = line[i];
      if (ch === '\\' && line[i + 1] === ' ') {
        token += ' ';
        i++;
      } else if (ch === ' ' || ch === '\t') {
        if (token) deps.push(token);
        token = '';
      } else token += ch;
    }
    if (token) deps.push(token);
  }
  return [...new Set(deps)];
};

const cargoHome = () => resolve(process.env.CARGO_HOME || join(homedir(), '.cargo'));

/** The repository files (relative, sorted) that the build read: dependency-file entries below the root, outside `dist/` and `out/`. Directories
 * (a build script's `rerun-if-changed` on `devices/`) are skipped; registry crates are pinned by Cargo.lock. */
export const repoFilesOfDepfile = (text, workspaceRoot) => {
  const names = new Set();
  for (const dep of parseDepfile(text)) {
    const path = resolve(dep);
    const rel = relative(workspaceRoot, path);
    if (rel.startsWith('..') || isAbsolute(rel)) {
      if (path.startsWith(cargoHome() + sep)) continue;
      throw new Error(`${DEPFILE} lists ${dep}, outside the repository and ${cargoHome()}`);
    }
    if (rel === 'dist' || rel.startsWith('dist/') || rel === 'out' || rel.startsWith('out/')) continue;
    if (!existsSync(path)) throw new Error(`${DEPFILE} lists ${rel}, which does not exist: build patchkit again`);
    if (statSync(path).isDirectory()) continue;
    names.add(rel.split(sep).join('/'));
  }
  return [...names].sort();
};

const expand = (globs, workspaceRoot) => globs.flatMap((glob) => globSync(glob, { cwd: workspaceRoot }));

// The Cargo.toml of the crate that holds each source file, the root manifest and Cargo.lock, and `.cargo/`: features and profiles are not in the dependency file.
const buildFiles = (sources, workspaceRoot) => {
  const files = new Set(['Cargo.toml', 'Cargo.lock']);
  for (const source of sources) {
    for (let dir = dirname(source); dir !== '.'; dir = dirname(dir))
      if (existsSync(join(workspaceRoot, dir, 'Cargo.toml'))) {
        files.add(`${dir}/Cargo.toml`);
        break;
      }
  }
  if (existsSync(join(workspaceRoot, '.cargo')))
    for (const file of walkFiles(join(workspaceRoot, '.cargo'), { strict: true }))
      files.add(relative(workspaceRoot, file).split(sep).join('/'));
  return files;
};

/** The key text of each device (a Map short -> text). `facts`: { rustc, profileEnv, rustflags }, strings. */
export const patchkitKeys = ({ depText, workspaceRoot = root, facts }) => {
  const read = repoFilesOfDepfile(depText, workspaceRoot);
  if (!read.some((name) => name.startsWith('packages/patchkit/')))
    throw new Error(`${DEPFILE} lists no patchkit source: it is not the dependency file of the patchkit build`);
  // A test source is read by the build (rustc reads a `#[cfg(test)] mod x;` file) but not compiled: the key leaves it out.
  // The cfg(test) guard of tools/nx/test-source.mjs (a host test) proves that the exclusion is honest.
  const sources = read.filter((name) => !isTestSource(name));
  const keys = new Map();
  for (const device of deviceShorts(workspaceRoot)) {
    const others = new Set(expand(otherFiles(device, workspaceRoot), workspaceRoot));
    const names = new Set([
      ...sources.filter((name) => !others.has(name)),
      ...buildFiles(sources, workspaceRoot),
      ...expand(filesOf(device, workspaceRoot), workspaceRoot), // its ports and peer file are read at run time, not embedded
    ]);
    keys.set(
      device,
      [
        `# patchkit-test-key 1 ${device}`,
        `# rustc ${facts.rustc.trim().replace(/\n/g, '; ')}`,
        `# profile ${facts.profileEnv}`,
        `# rustflags ${facts.rustflags}`,
        ...[...names].sort().map((name) => `${sha256File(join(workspaceRoot, name))}  ${name}`),
      ].join('\n') + '\n',
    );
  }
  return keys;
};

/** Writes the key files and removes the ones of devices that are gone; a file with the same content is left alone. */
export const writeKeys = (keys, workspaceRoot = root) => {
  const dir = join(workspaceRoot, KEY_DIR);
  mkdirSync(dir, { recursive: true });
  const wanted = new Set([...keys.keys()].map((d) => basename(keyFile(d))));
  for (const name of readdirSync(dir))
    if (/^patchkit-test-.*\.key$/.test(name) && !wanted.has(name)) rmSync(join(dir, name));
  for (const [device, text] of keys) {
    const path = join(workspaceRoot, keyFile(device));
    if (existsSync(path) && readFileSync(path, 'utf8') === text) continue;
    writeFileSync(`${path}.tmp`, text);
    renameSync(`${path}.tmp`, path);
  }
};

const toolchainFacts = () => {
  const done = spawnSync('rustc', ['-vV'], { encoding: 'utf8' }); // the rustc of the build step: same environment
  if (done.status !== 0) throw new Error(`rustc -vV failed: ${done.stderr || done.error}`);
  const flags = ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'RUSTC_WRAPPER']
    .map((k) => `${k}=${process.env[k] ?? ''}`)
    .join(' ');
  return { rustc: done.stdout, profileEnv: releaseProfileEnv(), rustflags: flags };
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const depfile = join(root, DEPFILE);
    if (!existsSync(depfile)) throw new Error(`${DEPFILE} is missing: build patchkit first (patchkit:cli)`);
    writeKeys(patchkitKeys({ depText: readFileSync(depfile, 'utf8'), facts: toolchainFacts() }));
  } catch (error) {
    console.error(`patchkit-key: ${error.message}`);
    process.exit(1);
  }
}
