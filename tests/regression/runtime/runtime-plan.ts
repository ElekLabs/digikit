import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { repoRoot, resolveInputs, type Session, type SessionOptions } from '@digi/test-support';

const exec = promisify(execFile);
const root = repoRoot();
export interface RuntimePlan {
  base: { device: string; version: string; syx_sha256: string };
  modules: string[];
  symbols: Record<string, number>;
  output: { syx_sha256: string } | null;
}

/** Same exact-one-address semantics as pk_harness.plan_symbol, including Rust statics. */
export function planSymbol(plan: RuntimePlan, name: string): number {
  const addresses = new Set(
    Object.entries(plan.symbols)
      .filter(([key]) => key.endsWith(`:${name}`) || new RegExp(`\\d${name}$`).test(key))
      .map(([, value]) => value),
  );
  if (addresses.size !== 1) throw new Error(`symbol ${name}: ${addresses.size} addresses in the plan`);
  return [...addresses][0]!;
}

export function validateRuntimePlan(plan: RuntimePlan, session: Pick<Session, 'options' | 'loadResult'>): void {
  const identity = session.loadResult.runtime;
  // Cache hits report plain IDs in root order; misses report tags in dependency order.
  const moduleIds = (modules: string[]) => modules.map((module) => module.split('@', 1)[0]).sort();
  if (
    !identity?.patched ||
    identity.set !== (session.options.runtime?.patchSet ?? 'emulator') ||
    plan.base.device !== session.options.device ||
    plan.base.version !== session.options.firmware ||
    plan.base.syx_sha256 !== identity.base_sha256 ||
    JSON.stringify(moduleIds(plan.modules)) !== JSON.stringify(moduleIds(identity.modules))
  ) {
    throw new Error('runtime patch plan does not match the loaded runtime identity and module membership');
  }
  // Exact loaded-image identity proves version, ordering and symbol layout despite report differences.
  const loadedSha256 = session.loadResult.syx_sha256;
  if (
    typeof loadedSha256 !== 'string' ||
    !/^[a-f0-9]{64}$/.test(loadedSha256) ||
    plan.output?.syx_sha256 !== loadedSha256
  ) {
    throw new Error('runtime patch plan output hash does not match the loaded runtime image');
  }
}

export interface PreparedRuntimePlan {
  plan: RuntimePlan;
  planFile: string;
  syx: string;
}

/** Offline plan preparation also supplies startup instrumentation before a daemon exists. */
export async function prepareRuntimePlanOffline(
  options: Pick<SessionOptions, 'device' | 'firmware' | 'runtime'>,
  directory: string,
  artifacts: string,
  symbols: string[],
): Promise<PreparedRuntimePlan> {
  const { device, firmware } = options;
  fs.mkdirSync(directory, { recursive: true });
  const syx = path.join(directory, `runtime-${device}.syx`);
  const planFile = syx.replace(/\.syx$/, '.plan.json');
  const patch = await exec(
    'uv',
    [
      'run',
      'python',
      '-m',
      'tools.patchkit.patchkit',
      'patch',
      '--firmware',
      resolveInputs(device, firmware).syx,
      '--set',
      options.runtime?.patchSet ?? 'emulator',
      '-o',
      syx,
      '--plan',
      planFile,
    ],
    { cwd: root, timeout: 30_000, maxBuffer: 4 * 1024 * 1024, env: { ...process.env, UV_NO_SYNC: '1' } },
  );
  fs.writeFileSync(path.join(artifacts, 'patch.log'), `${patch.stdout}\n${patch.stderr}`);
  const plan: RuntimePlan = JSON.parse(fs.readFileSync(planFile, 'utf8'));
  for (const symbol of symbols) planSymbol(plan, symbol);
  return { plan, planFile, syx };
}

/** Loaded-image identity is checked separately from offline preparation. */
export async function prepareRuntimePlan(session: Session, artifacts: string, symbols: string[]): Promise<RuntimePlan> {
  const { plan } = await prepareRuntimePlanOffline(session.options, session.runDir, artifacts, symbols);
  validateRuntimePlan(plan, session);
  return plan;
}
