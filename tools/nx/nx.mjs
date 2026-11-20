// The one launcher for Nx (`pnpm nx:pilot`): one CPU budget, one shared lock, one priority.
// Nx has no per-kind limits, so the per-target counts (Vitest workers, emulators, cargo jobs)
// come from this budget and reach the targets through environment variables.
import { spawn } from 'node:child_process';
import { createWriteStream, mkdirSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { MODE_CORES, budget, workCores } from './resources.mjs';
import { NxCompactor } from './failures.mjs';

export function runsTasks(args) {
  const first = args[0];
  const runs =
    first === 'run' || first === 'run-many' || first === 'affected' || (first?.includes(':') && !first.startsWith('-'));
  if (!runs) return false;
  return !args.some((arg) => ['--graph', '--explain', '--help'].some((flag) => arg === flag || arg.startsWith(flag)));
}

export const LIGHT_TARGETS = new Set([
  'lint',
  'fmt-check',
  'rust-fmt',
  'typecheck',
  'test-host',
  'check',
  'check:ui',
  'build:ui',
  'python-checks',
]);

function targetsOf(args) {
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    for (const flag of ['-t', '--targets']) {
      if (arg === flag) return args[i + 1]?.split(',').filter(Boolean) ?? [];
      if (arg.startsWith(`${flag}=`))
        return arg
          .slice(flag.length + 1)
          .split(',')
          .filter(Boolean);
    }
  }
  const spec = args[0] === 'run' ? args[1] : args[0];
  if (spec?.includes(':') && !spec.startsWith('-')) return [spec.slice(spec.lastIndexOf(':') + 1)];
  return [];
}

export function needsLock(args) {
  if (!runsTasks(args)) return false;
  const targets = targetsOf(args);
  return targets.length === 0 || !targets.every((target) => LIGHT_TARGETS.has(target));
}

// Plain launcher runs read and write the shared remote cache when its server already answers; they
// never start it. `remote-cache.mjs exec` (the gate) sets the Nx variables first and wins.
export function usesRemote(args, env) {
  return (
    runsTasks(args) &&
    env.DIGI_NX_REMOTE !== '0' &&
    !env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER &&
    !args.some((arg) => arg === '--skip-nx-cache' || arg.startsWith('--skip-nx-cache='))
  );
}

// Task runs print compact output: one line per task and the first failure block of each failed task
// (tools/nx/failures.mjs); the full output goes to out/runs/nx/<stamp>.log. `--verbose` or DIGI_NX_VERBOSE=1
// keeps Nx's own output, as does any other `--output-style` or NX_DEFAULT_OUTPUT_STYLE.
export function compactArgs(args, env) {
  const verbose = args.includes('--verbose') || (env.DIGI_NX_VERBOSE && env.DIGI_NX_VERBOSE !== '0');
  const style = args.find((arg) => arg.startsWith('--output-style'));
  if (!runsTasks(args) || verbose || env.NX_DEFAULT_OUTPUT_STYLE) return undefined;
  if (style && style !== '--output-style=static-failures-only') return undefined;
  if (style) return args;
  const split = args.indexOf('--');
  const at = split < 0 ? args.length : split;
  return [...args.slice(0, at), '--output-style=static-failures-only', ...args.slice(at)];
}

function main() {
  const args = process.argv.slice(2);
  const cwd = process.cwd();
  let b;
  try {
    b = budget(workCores(), process.env.DIGI_BUDGET);
  } catch (error) {
    console.error(`nx: ${error.message}`);
    process.exit(2);
  }
  const env = {
    ...process.env,
    NX_CACHE_DIRECTORY: `${cwd}/.nx/cache`,
    NX_WORKSPACE_DATA_DIRECTORY: `${cwd}/.nx/workspace-data`,
    NX_DAEMON: 'false',
    NX_NO_CLOUD: 'true',
  };
  env.NX_PARALLEL ??= process.env.DIGI_NX_PARALLEL || String(b.nxTasks);
  env.REGRESSION_JOBS ??= String(b.workers);
  env.DIGI_JOBS ??= String(b.jobs);
  env.CARGO_BUILD_JOBS ??= String(b.budget);
  env.NEXTEST_TEST_THREADS ??= String(b.threads);

  let command = ['pnpm', 'exec', 'nx', ...args];
  if (runsTasks(args)) {
    const lock = ['uv', 'run', 'python', '-m', 'tools.devtools.benchlock', 'run', '--shared', '--label', 'nx', '--'];
    command = [
      ...(process.platform === 'win32' ? [] : ['nice', '-n', '10']),
      ...(needsLock(args) ? lock : []),
      ...command,
    ];
    const remote = usesRemote(args, env);
    if (remote)
      command = [
        'node',
        fileURLToPath(new URL('remote-cache.mjs', import.meta.url)),
        'exec',
        '--access',
        'rw',
        '--no-start',
        '--',
        ...command,
      ];
    console.error(
      `nx: budget ${b.budget}: ${env.NX_PARALLEL} Nx tasks, ${b.workers} vitest workers, ${b.tasks} emulators (at most ${Math.floor(b.budget / MODE_CORES.sharc)} coupled), ${b.jobs} script jobs, ${b.budget} cargo jobs${remote ? ', remote' : ''}`,
    );
  }
  const compact = compactArgs(args, process.env);
  if (!compact) {
    const child = spawn(command[0], command.slice(1), { stdio: 'inherit', env });
    child.on('error', (error) => {
      throw error;
    });
    child.on('close', (status) => process.exit(status ?? 1));
    return;
  }
  // The same command with the quiet style; the output is read through the filter and kept whole in the log.
  const at = command.length - args.length;
  const full = [...command.slice(0, at), ...compact];
  const dir = `${cwd}/out/runs/nx`;
  mkdirSync(dir, { recursive: true });
  const logPath = `${dir}/${new Date().toISOString().replace(/[:.]/g, '-')}-${process.pid}.log`;
  const log = createWriteStream(logPath);
  const started = Date.now();
  const filter = new NxCompactor({
    print: (line) => console.log(line),
    logPath,
    elapsed: () => (Date.now() - started) / 1000,
  });
  const child = spawn(full[0], full.slice(1), { stdio: ['inherit', 'pipe', 'pipe'], env });
  for (const stream of [child.stdout, child.stderr]) {
    stream.setEncoding('utf8');
    stream.on('data', (chunk) => {
      log.write(chunk);
      filter.push(chunk);
    });
  }
  child.on('error', (error) => {
    throw error;
  });
  child.on('close', (status) => {
    filter.end(status ?? 1);
    log.end(() => process.exit(status ?? 1));
  });
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) main();
