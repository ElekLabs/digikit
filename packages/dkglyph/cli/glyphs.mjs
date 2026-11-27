#!/usr/bin/env node
// Glyph authoring without a device. SET is a set directory (default sdk/glyphs/digikit).
//
//     node packages/dkglyph/cli/glyphs.mjs check [SET]                    errors, warnings, coverage; exit 1 on errors
//     node packages/dkglyph/cli/glyphs.mjs sheet [SET] [-o PNG]           contact sheet, default out/glyphs/<set>.png
//     node packages/dkglyph/cli/glyphs.mjs json [SET]                     the built set as JSON on stdout
//     node packages/dkglyph/cli/glyphs.mjs dev [SET] [--no-open] [--port N]   Vite preview that reloads when a glyph file changes
import { execFile, spawn } from 'node:child_process';
import { mkdirSync, watch, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkSet, loadSet, setJson, sheetPng } from '../src/index.ts';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const usage =
  'usage: node packages/dkglyph/cli/glyphs.mjs check [SET] | sheet [SET] [-o PNG] | dev [SET] [--no-open] [--port N]';

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const flags = { open: true, port: undefined, out: undefined };
  const positional = [];
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--no-open') flags.open = false;
    else if (a === '--port') flags.port = Number(argv[++i]);
    else if (a === '-o') flags.out = argv[++i];
    else if (a.startsWith('-')) fail(`unknown option ${a}\n${usage}`);
    else positional.push(a);
  }
  return { flags, set: path.resolve(positional[0] ?? path.join(root, 'sdk/glyphs/digikit')) };
}

function printReport(set, report) {
  for (const m of report.errors) console.log(`error: ${m.id}: ${m.text}`);
  for (const m of report.warnings) console.log(`warning: ${m.id}: ${m.text}`);
  const loaded = set.glyphs.filter((g) => g.rendered).length;
  console.log(
    `${set.name}: ${set.glyphs.length} glyph files, ${loaded} built, ${report.errors.length} errors, ${report.warnings.length} warnings`,
  );
  const other = report.missing.filter((id) => !report.missingV1.includes(id));
  console.log(
    `coverage: ${set.catalogue.length - report.missing.length} of ${set.catalogue.length} catalogue entries have a file`,
  );
  if (report.missingV1.length) console.log(`missing v1: ${report.missingV1.join(', ')}`);
  if (other.length) console.log(`missing: ${other.join(', ')}`);
}

async function check(dir) {
  const set = await loadSet(dir);
  const report = checkSet(set);
  printReport(set, report);
  process.exit(report.errors.length ? 1 : 0);
}

async function sheet(dir, out) {
  const set = await loadSet(dir);
  const file = path.resolve(out ?? path.join(root, 'out/glyphs', `${set.name}.png`));
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, sheetPng(set));
  console.log(`wrote ${path.relative(process.cwd(), file) || file}`);
}

async function json(dir) {
  const set = await loadSet(dir);
  const report = checkSet(set);
  const errors = set.glyphs.filter((g) => g.error).map((g) => ({ id: g.id, ...g.error }));
  process.stdout.write(JSON.stringify({ ...setJson(set), report, errors, catalogue: set.catalogue }));
}

function openBrowser(url) {
  const [cmd, ...a] =
    process.platform === 'darwin'
      ? ['open', url]
      : process.platform === 'win32'
        ? ['cmd', '/c', 'start', '', url]
        : ['xdg-open', url];
  spawn(cmd, a, { stdio: 'ignore', detached: true })
    .on('error', () => {})
    .unref();
}

async function dev(dir, flags) {
  const { createServer } = await import('vite');
  // A fresh process per build: Node's ESM cache would keep stale helper modules.
  const build = () =>
    new Promise((resolve) => {
      execFile(
        process.execPath,
        [fileURLToPath(import.meta.url), 'json', dir],
        { cwd: root, env: process.env, maxBuffer: 256 * 1024 * 1024 },
        (err, stdout, stderr) => {
          if (err) {
            const message = (stderr || err.message).trim();
            resolve(JSON.stringify({ error: message, errors: [{ id: '(set)', message }] }));
          } else resolve(stdout);
        },
      );
    });
  let cached;
  let pending;
  const getBody = () => {
    if (cached !== undefined) return Promise.resolve(cached);
    pending ??= build().then((body) => {
      cached = body;
      pending = undefined;
      return body;
    });
    return pending;
  };
  const plugin = {
    name: 'glyphs-dev',
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        if ((req.url ?? '').split('?')[0] !== '/set.json') return next();
        const body = await getBody();
        res.setHeader('content-type', 'application/json');
        res.setHeader('cache-control', 'no-store');
        res.end(body);
      });
      let timer;
      watch(dir, { recursive: true }, (_event, name) => {
        if (name && !/\.(mjs|toml)$/.test(name)) return;
        clearTimeout(timer);
        timer = setTimeout(() => {
          console.log(`changed: ${name ?? dir}`);
          cached = undefined;
          server.ws.send({ type: 'custom', event: 'glyphs:update', data: {} });
        }, 50);
      }).unref();
    },
  };
  const server = await createServer({
    root: path.join(root, 'packages/dkglyph/preview'),
    configFile: false,
    plugins: [plugin],
    server: { fs: { allow: [root] }, ...(flags.port ? { port: flags.port, strictPort: true } : {}) },
  });
  await server.listen();
  server.printUrls();
  const url = server.resolvedUrls?.local[0];
  if (flags.open && url) openBrowser(url);
}

const [command = 'check', ...rest] = process.argv.slice(2);
const { flags, set } = parseArgs(rest);
if (command === 'check') await check(set);
else if (command === 'sheet') await sheet(set, flags.out);
else if (command === 'json') await json(set);
else if (command === 'dev') await dev(set, flags);
else fail(`unknown command ${command}\n${usage}`);
