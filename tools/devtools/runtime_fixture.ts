// Patched-runtime replay fixture: one genuine patched cold boot of the current runtime, saved as
// the fixed input set of `coupled-run` (two-core PGO training, timing matrix cells).
//
//   node tools/devtools/runtime_fixture.ts --device dn2|dt2 [--out DIR] [--binary DIGI_EMUD]
//        [--patch-set emulator] [--samples DIR]
//
// Output (default out/fixtures/runtime/<device>/): firmware.syx (patched), dsp-image.bin,
// dsp-state.snap.dsp, cf-snapshot.snap, card.img (DT2 only) and provenance.json.
// Replay: coupled-run --syx firmware.syx --dsp-image dsp-image.bin --dsp-state dsp-state.snap.dsp
//         --cf-snapshot cf-snapshot.snap [--card-image card.img]
// The daemon is the plain build of the device's generated core: --binary, else DIGI_EMUD_<DEV>,
// else `tools.emulator.native_emu --daemon --build-only` (a no-op when the build is fresh).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { resolveInputs, repoRoot, type Device } from '../../packages/test-support/src/inputs.ts';
import { Session } from '../../packages/test-support/src/session.ts';

const FIRMWARE: Record<Device, string> = { dt2: '1.17', dn2: '1.12' };
const BOOT_LIMIT_MS = 180_000;

function sha256File(file: string): string {
  const hash = createHash('sha256');
  const fd = fs.openSync(file, 'r');
  const buf = Buffer.allocUnsafe(8 << 20);
  try {
    for (let n; (n = fs.readSync(fd, buf, 0, buf.length, null)) > 0;) hash.update(buf.subarray(0, n));
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest('hex');
}

function git(root: string, ...args: string[]): string {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
}

function daemonBinary(root: string, device: Device, syx: string, given?: string): string {
  const named = given ?? process.env[`DIGI_EMUD_${device.toUpperCase()}`];
  if (named) return path.resolve(named);
  const out = execFileSync(
    'uv',
    ['run', 'python', '-m', 'tools.emulator.native_emu', '--daemon', '--build-only', syx],
    { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
  const last = out
    .split('\n')
    .filter((l) => l.trim())
    .pop();
  if (!last || !fs.existsSync(last)) throw new Error(`native_emu --build-only printed no binary: ${out.slice(-300)}`);
  return last;
}

const { values } = parseArgs({
  options: {
    device: { type: 'string' },
    out: { type: 'string' },
    binary: { type: 'string' },
    'patch-set': { type: 'string', default: 'emulator' },
    samples: { type: 'string' },
  },
});
if (values.device !== 'dn2' && values.device !== 'dt2') {
  throw new Error('usage: runtime_fixture.ts --device dn2|dt2 [--out DIR] [--binary DIGI_EMUD] [--samples DIR]');
}
const device: Device = values.device;
const firmware = FIRMWARE[device];
const patchSet = values['patch-set'] ?? 'emulator';
const root = repoRoot();
const outDir = path.resolve(root, values.out ?? path.join('out', 'fixtures', 'runtime', device));
const tmpDir = `${outDir}.tmp-${process.pid}`;
const runDir = `${outDir}.run-${process.pid}`;
const samples = device === 'dt2' ? path.resolve(root, values.samples ?? 'out/runs/patchkit/samples') : undefined;
if (samples && !fs.existsSync(samples)) {
  throw new Error(`${samples} is missing (tools.emulator.card_check.make_samples makes the DT2 card samples)`);
}

const inputs = resolveInputs(device, firmware);
const binary = daemonBinary(root, device, inputs.syx, values.binary);
fs.rmSync(tmpDir, { recursive: true, force: true });
fs.mkdirSync(tmpDir, { recursive: true });
fs.mkdirSync(runDir, { recursive: true });

let session: Session | undefined;
try {
  const started = Date.now();
  session = await Session.start({
    device,
    firmware,
    dsp: 'sharc',
    binary,
    runDir,
    ...(samples ? { card: samples } : {}),
    runtime: { patchSet },
  });
  const bootSeconds = (Date.now() - started) / 1000;
  await session.client.call('run.mode', { mode: 'paused' });
  const load = session.loadResult;
  const status = await session.status();
  if (load.start_used !== 'cold') throw new Error(`expected a cold boot, got start_used=${load.start_used}`);
  if (!load.runtime?.patched || load.runtime.set !== patchSet) throw new Error('boot is not the patched runtime');
  if (!status.cf.ready || status.cf.error) throw new Error(`firmware not ready: phase=${status.cf.phase}`);
  if (Date.now() - started > BOOT_LIMIT_MS) throw new Error('boot exceeded its limit');
  const cacheDir = String((load as unknown as { cache_dir?: string }).cache_dir ?? '');
  const snap = path.join(cacheDir, 'digi-audio-m5.snap');
  const snapDsp = `${snap}.dsp`;
  const patchedSyx = load.runtime.cache_file;
  const card = session.cardPath;
  for (const f of [snap, snapDsp, patchedSyx, ...(card ? [card] : [])]) {
    if (!f || !fs.existsSync(f)) throw new Error(`missing boot output: ${f}`);
  }

  const files = {
    firmware: 'firmware.syx',
    dsp_image: 'dsp-image.bin',
    dsp_state: 'dsp-state.snap.dsp',
    cf_snapshot: 'cf-snapshot.snap',
    card: card ? 'card.img' : null,
  };
  const sources: Record<string, string | undefined> = {
    firmware: patchedSyx,
    dsp_image: inputs.dspImage,
    dsp_state: snapDsp,
    cf_snapshot: snap,
    card,
  };
  const sha256: Record<string, string | null> = {};
  for (const [key, name] of Object.entries(files)) {
    if (name === null) {
      sha256[key] = null;
      continue;
    }
    // A clone on APFS (the card is 3.8 GB), else a plain copy.
    fs.copyFileSync(sources[key]!, path.join(tmpDir, name), fs.constants.COPYFILE_FICLONE);
    sha256[key] = sha256File(path.join(tmpDir, name));
  }
  const provenance = {
    schema: 1,
    device,
    firmware_version: firmware,
    patch_set: patchSet,
    files,
    sha256,
    source_head: git(root, 'rev-parse', 'HEAD'),
    created: new Date().toISOString(),
    // extra
    source_dirty: git(root, 'status', '--porcelain', '--untracked-files=no') !== '',
    stock_sha256: load.runtime.base_sha256,
    stock_firmware: path.basename(inputs.syx),
    patched_syx: path.basename(patchedSyx),
    runtime_modules: load.runtime.modules,
    dsp_core: status.audio?.dsp_core ?? null,
    daemon: { binary, sha256: sha256File(binary) },
    boot: { start_used: load.start_used, wall_seconds: bootSeconds, icount: load.icount },
  };
  fs.writeFileSync(path.join(tmpDir, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`);
  await session.close();
  session = undefined;
  fs.rmSync(outDir, { recursive: true, force: true });
  fs.renameSync(tmpDir, outDir);
  console.log(`runtime fixture ${device}: ${outDir} (boot ${bootSeconds.toFixed(1)} s)`);
} catch (err) {
  fs.rmSync(tmpDir, { recursive: true, force: true });
  throw err;
} finally {
  if (session) await session.close();
  fs.rmSync(runDir, { recursive: true, force: true });
}
