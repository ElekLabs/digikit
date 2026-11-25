// Prints one sha256 hex line over the untracked inputs of the coupled-run FX targets (gate-fx-dn2, gate-fx-dt2):
// the firmware and the private ready inputs that `tools/emulator/coupled.py` boots from.
// They are not the patchkit ones that tools/nx/device-inputs.mjs hashes. Mirrors tools/emulator/digi_env.py
// (`firmware`, `firmware_for`, `READY_DIRS`, `ready_inputs`, `[acceptance] ready_dir`); a change there must change this file.
// It also hashes the store key of each device's core library (the generated core is untracked and no Nx input sees it).
// The DT2 FX card is built from a generated sine, so it has no input file; DIGI_DT2_CARD (a kept card image) is hashed.
import { statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { sha256FileOrMissing, sha256Of } from './digest.mjs';
import { firmwareDir } from './device-inputs.mjs';
import { readyDirs } from './ready-dirs.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

const isFile = (p) => {
  try {
    return statSync(p).isFile();
  } catch {
    return false;
  }
};

// A bare environment value is relative to the repository root, as in digi_env.py.
const under = (p) => resolve(root, p);

export const coupledInputs = async (env = process.env) => {
  const lines = [];
  const add = async (label, file) =>
    lines.push(`${label} ${isFile(file) ? await sha256FileOrMissing(file) : 'missing'}`);

  await add('dn2/syx', env.DIGI_EMU_SYX || env.DIGI_COUPLED_SYX || join(firmwareDir(env), 'Digitone_II_OS1.12.syx'));
  await add('dt2/syx', env.DIGI_EMU_SYX_DT2 || join(firmwareDir(env), 'Digitakt_II_OS1.17.syx'));

  const dn2Ready = under(env.DIGI_EMU_READY_DIR || readyDirs.dn2);
  const dt2Ready = under(env.DIGI_EMU_READY_DIR_DT2 || readyDirs.dt2);
  await add('dn2/ready/dsp-image', env.DIGI_EMU_DSP_IMAGE || join(dn2Ready, 'digi-audio-dn2-image.bin'));
  await add('dn2/ready/dsp-state', env.DIGI_EMU_DSP_STATE || join(dn2Ready, 'digi-audio-m5.snap.dsp'));
  await add('dn2/ready/cf-snapshot', env.DIGI_EMU_CF_SNAPSHOT || join(dn2Ready, 'digi-audio-m5.snap'));
  for (const f of ['digi-audio-dn2-image.bin', 'digi-audio-m5.snap.dsp', 'digi-audio-m5.snap']) {
    await add(`dt2/ready/${f}`, join(dt2Ready, f));
  }
  if (env.DIGI_DT2_CARD) await add('dt2/card', under(env.DIGI_DT2_CARD));
  // The DSP core of each device is a library from the store: its key hashes the generated core (an untracked directory
  // that no Nx input sees), sharc-rt, the toolchain and the build plan (tools/devtools/core_store.py `key`).
  // An explicit DIGI_SHARC_CORE_LIB_<DEV> is hashed by content instead. The key is computed from `env`, the same
  // environment the gen dir and the explicit library are read from; it takes about 0.15 s and never builds.
  for (const device of ['dn2', 'dt2']) {
    const explicit = env[`DIGI_SHARC_CORE_LIB_${device.toUpperCase()}`];
    if (explicit) await add(`${device}/core-lib`, under(explicit));
    else {
      const done = spawnSync('uv', ['run', 'python', '-m', 'tools.devtools.core_store', 'key', '--device', device], {
        cwd: root,
        env,
        encoding: 'utf8',
        timeout: 60_000,
      });
      lines.push(`${device}/core-key ${done.status === 0 ? done.stdout.trim() : 'unavailable'}`);
    }
  }
  return sha256Of(`coupled-inputs v2\n${lines.join('\n')}`);
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) console.log(await coupledInputs());
