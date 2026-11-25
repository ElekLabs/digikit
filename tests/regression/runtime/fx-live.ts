import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { repoRoot, type Session, type Device, type RuntimeBaseline } from '@digi/test-support';
import { expect } from 'vitest';
import { withReusablePatchedSession } from '../support.ts';
import { record } from './artifacts.ts';
import { bounded } from './guest-access.ts';
import { plusdriveBinary } from './card-store.ts';
import { installSystemPackage } from './system-package.ts';
import { cachedCard, cardKey, fileSha } from './card-cache.ts';
import { daemonBinary, FIRMWARE, claimRunDir } from './devices.ts';
import { moduleArtifact } from './install.ts';
import { neededServices, portFile } from './service-files.ts';
import { deviceProfile } from './device-data.ts';
import { evidenceCollector, type RequireAudible } from './fx-evidence.ts';
import { planSymbol, prepareRuntimePlanOffline, type RuntimePlan } from './runtime-plan.ts';
import { runtimeSafety, runtimeWatchEnv, type SafetyPolicy } from './write-safety.ts';
import { GAIN, SINE } from './fixtures.ts';

const exec = promisify(execFile);
const root = repoRoot();
export function options(device: Device) {
  return { device, firmware: FIRMWARE[device], binary: daemonBinary(device), autoNo: true };
}

export function slotsFor(device: Device): number {
  return deviceProfile(device).limits.fx_slots;
}

/** Native plusdrive owns all card/store/selection encoding. The cards are built once into the card cache and reused by their inputs. */
async function baseline(
  session: Session,
  plan: RuntimePlan,
  wet: boolean,
  artifacts: string,
  cardServices: Set<string>,
): Promise<RuntimeBaseline> {
  const tag = wet ? 'wet' : 'dry';
  const binary = plusdriveBinary();
  if (!fs.existsSync(binary))
    throw new Error(
      `prepare prebuilt plusdrive separately: ${binary}; this fixture also needs prepared sine and gain DKMs`,
    );
  const device = session.options.device;
  const files = [SINE, GAIN].map((id) => {
    try {
      return { name: `${id.split('/')[1]}.dkm`, file: moduleArtifact(device, id) };
    } catch (error) {
      throw new Error(`FX reuse needs prepared ${id} and the prebuilt native plusdrive card builder; ${String(error)}`);
    }
  });
  const selection = ['modules/sine.dkm', ...(wet ? ['modules/dyn-fx-gain.dkm'] : [])];
  // the services the gain effect needs (fx.dks), go to digikit/services as packed, with the port file in digikit/ports
  const services = neededServices(files.map((f) => f.file)).map((needed) => ({
    name: (cardServices.add(needed.service), `${needed.service}.dks`),
    file: moduleArtifact(device, needed.moduleId),
  }));
  const port = services.length ? portFile(device) : undefined;
  const key = cardKey(device, {
    kind: 'fx-live-reuse',
    modules: files.map((f) => ({ name: f.name, sha: fileSha(f.file) })),
    services: services.map((f) => ({ name: f.name, sha: fileSha(f.file) })),
    port: port ? fileSha(port.file) : undefined,
    selection,
  });
  const cardImage = await cachedCard(key, async (work) => {
    // The seed stays in the card dir: tests read the built modules from it.
    const seed = path.join(work, 'card', 'seed');
    const modules = path.join(seed, 'digikit', 'modules');
    fs.mkdirSync(modules, { recursive: true });
    installSystemPackage(seed);
    for (const f of files) fs.copyFileSync(f.file, path.join(modules, f.name));
    const serviceDir = path.join(seed, 'digikit', 'services');
    fs.mkdirSync(serviceDir, { recursive: true });
    for (const f of services) fs.copyFileSync(f.file, path.join(serviceDir, f.name));
    if (port) {
      const portDir = path.join(seed, 'digikit', 'ports');
      fs.mkdirSync(portDir, { recursive: true });
      fs.copyFileSync(port.file, path.join(portDir, path.basename(port.name)));
    }
    fs.writeFileSync(path.join(seed, 'digikit', 'selection.txt'), selection.join('\n') + '\n');
    const out = path.join(work, 'card', 'card.img');
    fs.mkdirSync(path.dirname(out), { recursive: true });
    const built = await exec(binary, ['build', seed, '-o', out, '--device', device], { cwd: root, timeout: 30_000 });
    record(artifacts, `card-${tag}`, { stdout: built.stdout, stderr: built.stderr });
    return out;
  });
  const boot = planSymbol(plan, 'DKRT_BOOT');
  return {
    cardImage,
    // A targeted run-time invariant guard, not the legacy launch-time expansion write-watch/flush proof.
    guard: { protect: [{ lo: boot, hi: boot + 8, label: 'runtime boot identity' }], allow_pc: [] },
  };
}

/** After the owned emulators exit: every watch log and exported overlay meets the safety policy of the card's services. One call site for every session kind. */
export function expectLifetimeSafety(run: {
  device: Device;
  watchOutput: string;
  overlays: string[];
  epochs: Set<number>;
  artifacts: string;
  policy: SafetyPolicy;
  cardServices: Iterable<string>;
}) {
  const { device, watchOutput, overlays, epochs, artifacts, policy, cardServices } = run;
  const report = runtimeSafety(device, watchOutput, overlays, epochs.size, policy, [], [...cardServices]);
  record(artifacts, 'lifetime-safety', report);
  expect(report.pass, 'every emulator and exported overlay meets the write-safety policy').toBe(true);
}

export interface GainFxSessionOptions {
  initial?: 'dry' | 'wet';
  /** A patch set of the device file other than `emulator` (the sine and gain DKMs load as in any case). */
  patchSet?: string;
  /** Idle cases may be silent; guard, nonfinite, fault and overlay checks always remain mandatory. A suite that needs sound sets `true`. */
  requireAudible?: RequireAudible;
}

/** Fixed default-emulator sine/gain fixtures, sharing the existing owned Session lifecycle. */
export function withGainFxSession(device: Device, scope: string, config: GainFxSessionOptions = {}) {
  let plan: RuntimePlan;
  let dry: RuntimeBaseline;
  let wet: RuntimeBaseline;
  let artifacts: string;
  let closed = 0;
  let watchOutput: string;
  const overlays: string[] = [];
  const cardServices = new Set<string>();
  const epochs = new Set<number>();
  const rememberEpoch = (epoch: unknown) => {
    if (typeof epoch !== 'number') throw new Error('runtime status lacks epoch');
    epochs.add(epoch);
  };
  const slots = slotsFor(device);
  const collectEvidence = evidenceCollector(config, () => artifacts, overlays, rememberEpoch);
  const fixture = withReusablePatchedSession(
    {
      ...options(device),
      dsp: 'sharc', // effects run on the DSP
      patchSet: config.patchSet,
      runDirClaim: () => claimRunDir(device),
      env: async () => {
        artifacts = fs.mkdtempSync(path.join(os.tmpdir(), `digi-fx-reuse-${device}-`));
        console.info('FX safety artifacts:', artifacts);
        const prepared = await prepareRuntimePlanOffline(
          { device, firmware: FIRMWARE[device], runtime: { patchSet: config.patchSet } },
          artifacts,
          artifacts,
          ['DKRT_BOOT', 'fx.rows', '_dl_area'],
        );
        plan = prepared.plan;
        watchOutput = path.join(artifacts, 'writewatch.jsonl');
        const env = runtimeWatchEnv(prepared.planFile, watchOutput);
        record(artifacts, 'watch-environment', env);
        return env;
      },
    },
    {
      prepareBaseline: async (session) => {
        rememberEpoch((await bounded(() => session.status())).epoch);
        dry = await baseline(session, plan, false, artifacts, cardServices);
        wet = await baseline(session, plan, true, artifacts, cardServices);
        return config.initial === 'wet' ? wet : dry;
      },
      finishCase: (runtime) => collectEvidence(runtime, `case-${++closed}`),
      tempDirs: () => [artifacts],
      afterClose: async () => {
        expectLifetimeSafety({ device, watchOutput, overlays, epochs, artifacts, policy: 'default', cardServices });
      },
    },
    scope,
  );
  return {
    fixture,
    get plan() {
      return plan;
    },
    get artifacts() {
      return artifacts;
    },
    get slots() {
      return slots;
    },
    get dry() {
      return dry;
    },
    get wet() {
      return wet;
    },
    rememberEpoch,
    evidence: collectEvidence,
  };
}
