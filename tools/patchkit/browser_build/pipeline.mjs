// The browser module build (finding 75): `.dkmod` contents -> `.dkm`, with gcc cc1, as, ld, nm and objcopy as WebAssembly
// (wasm32-wasip1, tools/patchkit/gcc_wasm_build.sh) and patchkit.wasm for the steps patchkit does itself. It runs in a web worker
// and under node (same code). The steps and flags are those of `patchkit build` (packages/patchkit/src/build/cf.rs).
import { WASI, File, Directory, PreopenDirectory, OpenFile, ConsoleStdout } from './shim/index.js';

const enc = new TextEncoder();
const dec = new TextDecoder();
export const CFLAGS = [
  '-mcpu=54418',
  '-std=c11',
  '-Os',
  '-ffreestanding',
  '-fno-builtin',
  '-fno-pic',
  '-fno-pie',
  '-ffunction-sections',
  '-fdata-sections',
  '-fno-asynchronous-unwind-tables',
  '-fno-unwind-tables',
  '-fno-stack-protector',
  '-Wall',
  '-Wextra',
  '-Werror',
];
// the repository include directories (tools/patchkit/toolchain/pins.toml [include] cf), which the module's own `src` precedes
export const GLOBAL_INC = [
  'ports/bindings/inc',
  'ports/bindings/include',
  'runtime/core/include',
  'runtime/platform/ui/include',
  'sdk/include',
];

const norm = (path) =>
  path
    .split('/')
    .filter(Boolean)
    .reduce((a, p) => (p === '..' ? (a.pop(), a) : p === '.' ? a : (a.push(p), a)), []);

/** An in-memory tree that one WASI preopen ("/") serves. */
export class Fs {
  constructor() {
    this.root = new Directory(new Map());
  }
  dir(path, make) {
    let d = this.root;
    for (const p of norm(path)) {
      let n = d.contents.get(p);
      if (!n) {
        if (!make) return null;
        n = new Directory(new Map());
        d.contents.set(p, n);
      }
      d = n;
    }
    return d;
  }
  put(path, bytes) {
    path = '/' + norm(path).join('/');
    const i = path.lastIndexOf('/');
    this.dir(path.slice(0, i), true).contents.set(path.slice(i + 1), new File(bytes));
  }
  get(path) {
    path = '/' + norm(path).join('/');
    const i = path.lastIndexOf('/');
    const f = this.dir(path.slice(0, i), false)?.contents.get(path.slice(i + 1));
    if (!f) throw new Error(`no file ${path}`);
    return f.data;
  }
  list(path) {
    return [...(this.dir(path, false)?.contents.keys() ?? [])];
  }
}

export class Pipeline {
  /** `tools`: name -> compiled WebAssembly.Module; `log(step, ms, extra)` records a step. */
  constructor(tools, fs, log) {
    this.tools = tools;
    this.fs = fs;
    this.log = log;
    this.peakWasmMem = 0;
  }

  async run(tool, args) {
    const out = [];
    const err = [];
    const fds = [
      new OpenFile(new File(new Uint8Array(0))),
      ConsoleStdout.lineBuffered((l) => out.push(l)),
      ConsoleStdout.lineBuffered((l) => err.push(l)),
      new PreopenDirectory('/', this.fs.root.contents),
    ];
    const wasi = new WASI(['tool', ...args], [], fds);
    const inst = await WebAssembly.instantiate(this.tools[tool], { wasi_snapshot_preview1: wasi.wasiImport });
    const t0 = performance.now();
    const rc = wasi.start(inst);
    const ms = performance.now() - t0;
    this.peakWasmMem = Math.max(this.peakWasmMem, inst.exports.memory.buffer.byteLength);
    this.log(`${tool} ${args.find((a) => a.endsWith('.c') || a.endsWith('.s') || a.endsWith('.o')) ?? ''}`.trim(), ms, {
      mem: inst.exports.memory.buffer.byteLength,
    });
    if (rc !== 0) throw new Error(`${tool} failed (${rc}):\n${out.join('\n')}\n${err.join('\n')}`);
    return out.join('\n');
  }

  /** One C or assembly source to a relocatable object at `obj`. */
  async compile(src, obj, includeDirs) {
    if (src.endsWith('.s') || src.endsWith('.S')) return this.run('as', ['-mcpu=54418', '-o', obj, src]);
    const asm = obj.replace(/\.o$/, '.s');
    const base = src.slice(src.lastIndexOf('/') + 1);
    await this.run('cc1', [
      '-quiet',
      '-dumpbase',
      base,
      '-dumpbase-ext',
      '.c',
      ...CFLAGS,
      ...includeDirs.map((d) => `-I${d}`),
      '-isystem',
      '/gcc-include',
      src,
      '-o',
      asm,
    ]);
    return this.run('as', ['-mcpu=54418', '-o', obj, asm]);
  }

  async nm(obj, flags) {
    return (await this.run('nm', [...flags, obj]))
      .split('\n')
      .map((l) => l.trim().split(/\s+/).pop())
      .filter(Boolean);
  }

  /** `pk build`'s proto_of_sources for ColdFire C: compile, merge, link the SDK members the sources call, resolve libgcc helpers. */
  async protoElf(root, sources, includeDirs, sdkSources, libgcc) {
    const tmp = `${root}/tmp`;
    this.fs.dir(tmp, true);
    const objs = [];
    for (const [i, s] of sources.entries()) {
      const stem = s.slice(s.lastIndexOf('/') + 1).replace(/\.[^.]+$/, '');
      const o = `${tmp}/${i}_${stem}.o`;
      await this.compile(s, o, includeDirs);
      objs.push(o);
    }
    let merged = objs[0];
    if (objs.length > 1) {
      merged = `${tmp}/all.o`;
      await this.run('ld', ['-r', '-o', merged, ...objs]);
    }
    if (sdkSources.length && sources.some((s) => s.endsWith('.c'))) {
      const sdkObjs = [];
      for (const [i, s] of sdkSources.entries()) {
        const o = `${tmp}/sdk${i}.o`;
        await this.compile(
          s,
          o,
          GLOBAL_INC.map((d) => `${root}/${d}`),
        );
        sdkObjs.push(o);
      }
      await this.run('ld', ['-r', '-o', `${tmp}/sdk.o`, ...sdkObjs]);
      const globals = new Set(await this.nm(`${tmp}/sdk.o`, ['-g', '--defined-only']));
      const roots = [...new Set((await this.nm(merged, ['-u'])).filter((n) => globals.has(n)))].sort();
      if (roots.length) {
        await this.run('ld', [
          '-r',
          '--gc-sections',
          '-o',
          `${tmp}/sdk-part.o`,
          ...roots.flatMap((r) => ['-u', r]),
          `${tmp}/sdk.o`,
        ]);
        await this.run('ld', ['-r', '-o', `${tmp}/sdk-linked.o`, merged, `${tmp}/sdk-part.o`]);
        merged = `${tmp}/sdk-linked.o`;
      }
    }
    // the compiler helpers (__udivdi3, ...) from the ColdFire libgcc, made local
    const undef = await this.nm(merged, ['-u']);
    if (undef.length) {
      const helpers = new Set((await this.nm(libgcc, ['-g', '--defined-only'])).filter((n) => n.startsWith('__')));
      if (undef.some((n) => helpers.has(n))) {
        const before = new Set(await this.nm(merged, ['-g', '--defined-only']));
        await this.run('ld', ['-r', '-o', `${tmp}/helpers.o`, merged, libgcc]);
        const after = await this.nm(`${tmp}/helpers.o`, ['-g', '--defined-only']);
        const added = [...new Set(after)].filter((n) => !before.has(n)).sort();
        await this.run('objcopy', [
          '--remove-section=.eh_frame',
          ...added.map((n) => `--localize-symbol=${n}`),
          `${tmp}/helpers.o`,
          `${tmp}/helpers-local.o`,
        ]); // not in place: objcopy's temp file needs mkstemp, which WASI lacks
        merged = `${tmp}/helpers-local.o`;
      }
    }
    return this.fs.get(merged);
  }
}

// ---- patchkit.wasm (packages/patchkit/src/wasm.rs) ------------------------------------------------------------------------------

export class Patchkit {
  constructor(instance) {
    this.x = instance.exports;
  }
  put(bytes) {
    const p = this.x.pk_alloc(bytes.length);
    new Uint8Array(this.x.memory.buffer, p, bytes.length).set(bytes);
    return [p, bytes.length];
  }
  take(ptr, len) {
    return new Uint8Array(this.x.memory.buffer, this.x[ptr](), this.x[len]()).slice();
  }
  call(fn, ...args) {
    const flat = args.flatMap((a) => (typeof a === 'string' ? this.put(enc.encode(a)) : this.put(a)));
    const rc = this.x[fn](...flat);
    if (rc !== 0) throw new Error(`${fn}: ${dec.decode(this.take('pk_err_ptr', 'pk_err_len'))}`);
  }
  cfObject(elf, meta) {
    this.call('pk_cf_object', elf, JSON.stringify(meta));
    return dec.decode(this.take('pk_plan_ptr', 'pk_plan_len'));
  }
  moduleIds(manifest, id) {
    this.call('pk_module_ids', manifest, id);
    return dec.decode(this.take('pk_plan_ptr', 'pk_plan_len'));
  }
  sharcObject(input) {
    this.call('pk_sharc_object', JSON.stringify(input));
    return dec.decode(this.take('pk_plan_ptr', 'pk_plan_len'));
  }
  pack(bundle, id, device) {
    this.call('pk_pack', JSON.stringify(bundle), id, device);
    return this.take('pk_out_ptr', 'pk_out_len');
  }
}

// ---- the .dkmod zip --------------------------------------------------------------------------------------------------------------

/** Entries of a zip (stored or deflate): { name -> Uint8Array }. */
export async function unzip(buf) {
  const v = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let e = buf.length - 22;
  while (v.getUint32(e, true) !== 0x06054b50) e--;
  const n = v.getUint16(e + 10, true);
  let c = v.getUint32(e + 16, true);
  const files = {};
  for (let i = 0; i < n; i++) {
    const method = v.getUint16(c + 10, true);
    const csize = v.getUint32(c + 20, true);
    const nameLen = v.getUint16(c + 28, true);
    const extra = v.getUint16(c + 30, true);
    const cmt = v.getUint16(c + 32, true);
    const off = v.getUint32(c + 42, true);
    const name = dec.decode(buf.subarray(c + 46, c + 46 + nameLen));
    const data = off + 30 + v.getUint16(off + 26, true) + v.getUint16(off + 28, true);
    const raw = buf.subarray(data, data + csize);
    if (!name.endsWith('/')) {
      files[name] =
        method === 0
          ? raw.slice()
          : new Uint8Array(
              await new Response(
                new Blob([raw]).stream().pipeThrough(new DecompressionStream('deflate-raw')),
              ).arrayBuffer(),
            );
    }
    c += 46 + nameLen + extra + cmt;
  }
  return files;
}

/** The input bundle (one file: u32 index length, JSON { path: [offset, length] }, bytes) into the tree. */
export function unbundle(buf, fs) {
  const v = new DataView(buf.buffer, buf.byteOffset);
  const n = v.getUint32(0, true);
  const idx = JSON.parse(dec.decode(buf.subarray(4, 4 + n)));
  for (const [p, [o, l]] of Object.entries(idx)) fs.put(p, buf.slice(4 + n + o, 4 + n + o + l));
  return Object.keys(idx).length;
}

/**
 * The whole build: the `.dkmod` bytes to the `.dkm`. `root` is where the repository files sit in the tree (the include bundle puts them
 * there); the module's files go to `root/modules-in/<name>`.
 */
export async function buildDkm({ tools, patchkit, fs, dkmod, root, device, log, modBase = 'modules-in' }) {
  const pl = new Pipeline(tools, fs, log);
  const t0 = performance.now();
  const files = await unzip(dkmod);
  const pkg = dec.decode(files['package.toml']);
  const id = /^id\s*=\s*"([^"]+)"/m.exec(pkg)[1];
  const moddir = `${root}/${modBase}/${id.split('/').pop()}`;
  for (const [p, b] of Object.entries(files)) if (p !== 'package.toml') fs.put(`${moddir}/${p}`, b);
  log('unzip .dkmod', performance.now() - t0, {});
  const manifest = dec.decode(files['module.toml']);
  const objs = manifest
    .split(/^\[\[object\]\]/m)
    .slice(1)
    .map((b) => ({
      path: /^path\s*=\s*"([^"]+)"/m.exec(b)[1],
      sources: [...(/^sources\s*=\s*\[([^\]]*)\]/m.exec(b)?.[1] ?? '').matchAll(/"([^"]+)"/g)].map((m) => m[1]),
    }));
  const isSharc = (o) =>
    o.sources.every((s) => s.endsWith('.s')) && o.sources.some((s) => s.split('/').includes('dsp'));
  const ids = patchkit.moduleIds(manifest, id); // build/gen/module_ids.h, written by `patchkit gen` before a native build
  if (ids) fs.put(`${moddir}/build/gen/module_ids.h`, enc.encode(ids));
  const sdkSources = fs.list(`${root}/sdk/src`).length ? collect(fs, `${root}/sdk/src`).sort() : [];
  const dkos = {};
  let dko = '';
  for (const o of objs) {
    const t1 = performance.now();
    if (o.sources.length === 0) {
      dkos[o.path] = dec.decode(files[o.path]); // a payload file of the package (hex text), carried as it is
    } else if (isSharc(o)) {
      // SHARC+: Selache inside patchkit.wasm (pk_sharc_object); the include files by name, in the order of the native include path
      const dirs = [`${moddir}/build/gen`, `${moddir}/src`, `${root}/sdk/dsp/contract`, `${root}/sdk`];
      const includes = {};
      for (const d of dirs)
        for (const f of collectAll(fs, d)) includes[f.slice(d.length + 1)] ??= dec.decode(fs.get(f));
      for (const s of o.sources) {
        const sd = '/' + norm(`${moddir}/${s}`).slice(0, -1).join('/');
        for (const f of collectAll(fs, sd)) includes[f.slice(sd.length + 1)] ??= dec.decode(fs.get(f));
      }
      dkos[o.path] = patchkit.sharcObject({
        sources: o.sources.map((s) => dec.decode(fs.get(`${moddir}/${s}`))),
        includes,
        defines: [],
      });
      log('patchkit: SHARC+ source -> .dko', performance.now() - t1, {});
    } else {
      const sources = o.sources.map((s) => `${moddir}/${s}`);
      const incs = [`${moddir}/build/gen`, `${moddir}/src`, ...GLOBAL_INC.map((d) => `${root}/${d}`)];
      const elf = await pl.protoElf(root, sources, incs, sdkSources, '/libgcc-cf54418.a');
      const t2 = performance.now();
      dko = dkos[o.path] = patchkit.cfObject(elf, { source_sha256: '0'.repeat(64), toolchain: {} });
      log('patchkit: ELF -> .dko', performance.now() - t2, {});
    }
  }
  const t3 = performance.now();
  const dkm = patchkit.pack({ ports: [], modules: [{ manifest, files: dkos }] }, id, device);
  log('patchkit: pack .dkm', performance.now() - t3, {});
  return { id, dkm, dko, peakWasmMem: pl.peakWasmMem };
}

function collect(fs, dir) {
  const out = [];
  const d = fs.dir(dir, false);
  for (const [n, node] of d.contents) {
    if (node instanceof Directory) out.push(...collect(fs, `${dir}/${n}`));
    else if (n.endsWith('.c')) out.push(`${dir}/${n}`);
  }
  return out;
}

function collectAll(fs, dir) {
  const out = [];
  const d = fs.dir(dir, false);
  if (!d) return out;
  for (const [n, node] of d.contents) {
    if (node instanceof Directory) out.push(...collectAll(fs, `${dir}/${n}`));
    else if (/\.(inc|s|h)$/.test(n)) out.push(`${dir}/${n}`);
  }
  return out;
}
