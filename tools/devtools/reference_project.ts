import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { SamplesBackend } from '../../packages/dk-session/src/samples.ts';
import type { Registry } from '../../packages/devices/src/index.ts';
import type { SessionDocument } from '../../packages/workbench-core/src/host/session.ts';
import { buildCardImage, plusdrivePath, systemPackagePath } from '../../packages/workbench-core/src/node/card-image.ts';
import { firmwareDir } from '../../packages/workbench-core/src/node/stock-image.ts';
import { delayReference as fixture } from '../../packages/workbench-core/src/reference/dt2-stock-delay.ts';
import { delayPanelSteps, stageProbeSample } from '../../packages/workbench-core/src/reference/project-preparation.ts';
const supportModules = ['debug', 'remote'] as const;

interface ProjectOptions {
  root: string;
  registry: Registry;
  emulator: boolean;
  output?: string;
  firmware?: string;
  name?: string;
  wav: Buffer;
  openSampleTarget(): Promise<{ backend: SamplesBackend; close(): Promise<void> }>;
}

const hash = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
const json = (value: unknown) => JSON.stringify(value, null, 2) + '\n';

async function requireAsset(source: string, hint: string) {
  try {
    return await realpath(source);
  } catch {
    throw new Error(`Missing ${source}. ${hint}`);
  }
}

async function writeSession(options: ProjectOptions, destination: string, name: string) {
  const [id, version] = fixture.port.split('-');
  const { root } = options;
  const profile = options.registry.get(id!);
  const stock = profile.firmware.find((f) => f.version === version);
  if (!stock?.filename) throw new Error(`No registered stock firmware for ${fixture.port}.`);
  const firmware = await requireAsset(
    options.firmware ? path.resolve(options.firmware) : path.join(firmwareDir(root), stock.filename),
    'Pass --firmware FILE or set DIGI_FIRMWARE_DIR.',
  );
  if (hash(await readFile(firmware)) !== stock.sha256)
    throw new Error(`Expected the original ${fixture.port} firmware, got ${firmware}.`);
  await requireAsset(plusdrivePath(root), 'Run pnpm nx:pilot run plusdrive-format:cli.');
  const pkg = await requireAsset(systemPackagePath(root), 'Run pnpm nx:pilot run core/runtime:package.');
  const modules = await Promise.all(
    supportModules.map(async (module) => ({
      name: `modules/${module}.dkm`,
      file: await requireAsset(
        path.join(root, 'dist/modules', profile.id, `${module}.dkm`),
        `Run pnpm nx:pilot run digikit/${module}:pack.`,
      ),
    })),
  );
  await mkdir(path.dirname(destination), { recursive: true });
  await mkdir(destination); // Reserve a new folder. Never reuse a session or its card.
  await buildCardImage({
    root,
    device: profile.id,
    output: path.join(destination, 'card/base.img'),
    syx: firmware,
    store: modules,
    drive: [{ name: fixture.fixture, bytes: options.wav }],
    systemPackage: pkg,
    selection: modules.map((module) => module.name),
    timeoutMs: 90_000,
  });
  const attachment = `attachments/${fixture.fixture}`;
  await mkdir(path.join(destination, 'attachments'));
  await writeFile(path.join(destination, attachment), options.wav, { flag: 'wx' });
  const document: SessionDocument = {
    schema: 'dk.session/1',
    id: randomUUID(),
    name,
    firmware: {
      path: firmware,
      sha256: stock.sha256,
      device: profile.id,
      runtime: true,
      dsp: true,
      imageSha256: null,
    },
    checkpoint: null,
    attachments: [{ name: fixture.fixture, kind: 'audio', path: attachment }],
  };
  const session = path.join(destination, 'session.json');
  await writeFile(session, json(document), { flag: 'wx' });
  return session;
}

export async function prepareProject(options: ProjectOptions) {
  const name = options.name?.trim() || (options.emulator ? fixture.project.name : undefined);
  if (!name) throw new Error('Name the hardware scratch project with --scratch-project NAME.');
  if (options.emulator && !options.output) throw new Error('Choose a new Workbench session folder with --out DIR.');
  if (!options.emulator && options.firmware)
    throw new Error('--firmware selects the stock file for the emulator only.');
  if (hash(options.wav) !== fixture.fixtureSha256) throw new Error('The probe WAV does not match the project fixture.');
  const destination = options.output && path.resolve(options.output);
  const base = {
    schema: 'dk.reference-project/1',
    case: fixture.id,
    port: fixture.port,
    scratchProject: name,
    status: 'panel-setup-required',
    fixture: { file: fixture.fixture, sha256: fixture.fixtureSha256, ...fixture.project },
  } as const;
  if (options.emulator) {
    const session = await writeSession(options, destination!, name);
    const result = {
      ...base,
      target: 'emulator' as const,
      session,
      setupRemaining: delayPanelSteps('seeded', name, fixture.fixture),
    };
    await writeFile(path.join(destination!, 'preparation.json'), json(result), { flag: 'wx' });
    return result;
  }

  if (destination) {
    await mkdir(path.dirname(destination), { recursive: true });
    await mkdir(destination);
  }
  const link = await options.openSampleTarget();
  try {
    const sample = await stageProbeSample(link.backend, options.wav);
    const result = {
      ...base,
      target: 'hardware' as const,
      sample,
      setupRemaining: delayPanelSteps('unassigned', name, sample.destination),
    };
    if (destination) await writeFile(path.join(destination, 'preparation.json'), json(result), { flag: 'wx' });
    return result;
  } finally {
    await link.close();
  }
}
