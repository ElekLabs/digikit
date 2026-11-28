// Drive the browser module build in headless Chromium and compare each .dkm with the native `patchkit pack` output.
//   node tools/patchkit/browser_build/run.mjs --work out/gcc-wasm --ids test/c-demo,test/ui-views --device dt2 --patchkit patchkit.wasm --pk NATIVE_PATCHKIT [--root /path/in/the/guest]
// Each module is exported as a .dkmod (tools.patchkit.pk_package), built in the browser from its contents, and packed natively for the reference.
// Needs out/gcc-wasm/out/{cc1,as,ld,nm,objcopy}.wasm and include/ (gcc_wasm_build.sh), and in out/gcc-wasm/jsdeps:
//   npm i --save-exact @bjorn3/browser_wasi_shim@0.4.2 playwright-core@1.65.1
import { createServer } from 'node:http';
import { readFileSync, existsSync, writeFileSync, readdirSync, statSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { gzipSync, brotliCompressSync, constants } from 'node:zlib';
import { execFileSync } from 'node:child_process';
import path from 'node:path';

const args = Object.fromEntries(
  process.argv
    .slice(2)
    .reduce(
      (a, x, i, v) =>
        x.startsWith('--')
          ? [...a, [x.slice(2), v[i + 1]?.startsWith('--') || v[i + 1] === undefined ? true : v[i + 1]]]
          : a,
      [],
    ),
);
const here = path.dirname(new URL(import.meta.url).pathname);
const repo = path.resolve(here, '../../..');
const work = path.resolve(args.work);
const root = args.root ?? repo;
const sha = (b) => createHash('sha256').update(b).digest('hex');
const GLOBAL_INC = [
  'ports/bindings/inc',
  'ports/bindings/include',
  'runtime/core/include',
  'runtime/platform/ui/include',
  'sdk/include',
  'sdk/src',
  'sdk/dsp',
];

function walk(dir, out = []) {
  for (const n of readdirSync(dir).sort()) {
    const p = path.join(dir, n);
    statSync(p).isDirectory() ? walk(p, out) : out.push(p);
  }
  return out;
}

// the repository files the build reads, at their absolute paths under `root`, as one bundle
function makeBundle() {
  const files = {};
  // --fixtures: wrapper fixtures take sources from a sibling module (`../dyn-fx-gain/src/...`), which a .dkmod does not carry
  if (args.fixtures)
    for (const f of walk(path.join(repo, 'tests/fixtures/modules')))
      if (f.includes('/src/')) files[path.join(root, path.relative(repo, f))] = readFileSync(f);
  for (const d of GLOBAL_INC)
    for (const f of walk(path.join(repo, d))) files[path.join(root, path.relative(repo, f))] = readFileSync(f);
  for (const f of readdirSync(path.join(work, 'out/include')))
    files[`/gcc-include/${f}`] = readFileSync(path.join(work, 'out/include', f));
  files['/libgcc-cf54418.a'] = readFileSync(path.join(repo, 'out/tools/m68k-elf/lib/libgcc-cf54418.a'));
  const idx = {};
  let off = 0;
  const parts = [];
  for (const [p, b] of Object.entries(files)) {
    idx[p] = [off, b.length];
    off += b.length;
    parts.push(b);
  }
  const head = Buffer.from(JSON.stringify(idx));
  const n = Buffer.alloc(4);
  n.writeUInt32LE(head.length);
  return Buffer.concat([n, head, ...parts]);
}

const ids = String(args.ids).split(',');
const device = args.device ?? 'dt2';
const scratch = path.join(work, 'run');
mkdirSync(scratch, { recursive: true });
const dkmods = ids.map((id) => {
  const f = path.join(scratch, `${id.replace('/', '_')}.dkmod`);
  execFileSync('uv', ['run', 'python', '-m', 'tools.patchkit.pk_package', 'export', id, '-o', f], {
    cwd: repo,
    stdio: 'ignore',
  });
  return f;
});
const refs = ids.map((id) => {
  const f = path.join(scratch, `${id.replace('/', '_')}.native.dkm`);
  try {
    execFileSync(args.pk, ['--root', repo, 'pack', id, '--device', device, '-o', f], { stdio: 'ignore' });
  } catch {
    return null;
  }
  return f;
});
const bundle = makeBundle();
const assets = {
  '/inputs.bin': bundle,
  ...Object.fromEntries(dkmods.map((f, i) => [`/input-${i}.dkmod`, readFileSync(f)])),
  '/a/patchkit.wasm': readFileSync(args.patchkit),
  ...Object.fromEntries(
    ['cc1', 'as', 'ld', 'nm', 'objcopy'].map((n) => [
      `/a/${n}.wasm`,
      readFileSync(path.join(work, 'out', `${n}.wasm`)),
    ]),
  ),
};
const types = {
  '.mjs': 'text/javascript',
  '.js': 'text/javascript',
  '.html': 'text/html',
  '.wasm': 'application/wasm',
};
const shim = path.join(work, 'jsdeps/node_modules/@bjorn3/browser_wasi_shim/dist');
const server = createServer((req, res) => {
  const u0 = new URL(req.url, 'http://x').pathname;
  const u = u0 === '/' ? '/index.html' : u0;
  let body = assets[u];
  if (!body) {
    const f = u.startsWith('/shim/') ? path.join(shim, u.slice(6)) : path.join(here, u.slice(1));
    if (existsSync(f) && statSync(f).isFile()) body = readFileSync(f);
  }
  if (!body) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, {
    'content-type': types[path.extname(u)] ?? 'application/octet-stream',
    'x-wire-bytes': String(brotliCompressSync(body, { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length),
  });
  res.end(body);
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;

let result;
let peakRss = 0;
{
  const { chromium } = await import(path.join(work, 'jsdeps/node_modules/playwright-core/index.mjs'));
  const exe =
    args.chrome ??
    execFileSync('sh', [
      '-c',
      'ls -d ~/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-mac-arm64/chrome-headless-shell | tail -1',
    ])
      .toString()
      .trim();
  const marker = `--digi-run=${process.pid}`;
  const browser = await chromium.launch({ executablePath: exe, headless: true, args: [marker] });
  const page = await browser.newPage();
  page.on('console', (m) => {
    if (!m.text().includes('wasi:')) console.error('[page]', m.text());
  });
  page.on('pageerror', (e) => console.error('[pageerror]', e.message));
  const poll = setInterval(() => {
    try {
      const all = execFileSync('ps', ['-axo', 'pid=,ppid=,rss=,command=']).toString().trim().split('\n');
      const out = all.map((l) => l.trim().split(/\s+/).slice(0, 3).map(Number));
      const kids = new Set(all.filter((l) => l.includes(marker)).map((l) => Number(l.trim().split(/\s+/)[0])));
      for (let i = 0; i < 3; i++) for (const [p, pp] of out) if (kids.has(pp)) kids.add(p);
      peakRss = Math.max(
        peakRss,
        out.filter(([p]) => kids.has(p)).reduce((s, [, , r]) => s + r, 0),
      );
    } catch {}
  }, 200);
  const t0 = Date.now();
  await page.goto(
    `http://127.0.0.1:${port}/?root=${encodeURIComponent(root)}&device=${device}&count=${ids.length}${args.fixtures ? '&modBase=tests/fixtures/modules' : ''}`,
  );
  await page.waitForFunction('window.__result', null, { timeout: 20 * 60 * 1000, polling: 500 });
  result = await page.evaluate(() => window.__result);
  result.wall = Date.now() - t0;
  clearInterval(poll);
  result.browser = browser.version();
  await browser.close();
}
server.close();

if (!result.ok) {
  console.error(result.error);
  console.log(JSON.stringify(result.log));
  process.exit(1);
}
console.log(JSON.stringify(result.log));
let same = 0;
for (const [i, m] of result.modules.entries()) {
  if (m.error) {
    console.log(`FAILED ${ids[i]}: ${m.error.split('\n').slice(0, 3).join(' | ')}`);
    continue;
  }
  const dkm = Buffer.from(Object.values(m.dkm));
  if (!refs[i]) {
    console.log(`NO-NATIVE ${ids[i]}: native pack failed (browser built ${dkm.length} B)`);
    continue;
  }
  const ref = readFileSync(refs[i]);
  const ok = ref.equals(dkm);
  same += ok;
  writeFileSync(path.join(scratch, `${ids[i].replace('/', '_')}.browser.dkm`), dkm);
  writeFileSync(path.join(scratch, `${ids[i].replace('/', '_')}.browser.dko.json`), m.dko);
  console.log(
    `${ok ? 'IDENTICAL' : 'DIFFERENT'} ${ids[i]}: browser ${dkm.length} B ${sha(dkm).slice(0, 16)}, native ${ref.length} B ${sha(ref).slice(0, 16)}, build ${m.ms} ms, wasm mem ${(m.peakWasmMem / 1048576).toFixed(0)} MB`,
  );
}
console.log(
  JSON.stringify(
    {
      browser: result.browser,
      wall_ms: result.wall,
      peak_browser_rss_mb: +(peakRss / 1024).toFixed(0),
      identical: `${same}/${result.modules.length}`,
      bundle_bytes: bundle.length,
      bundle_gzip: gzipSync(bundle, { level: 9 }).length,
      bundle_brotli: brotliCompressSync(bundle).length,
    },
    null,
    1,
  ),
);

process.exit(0);
