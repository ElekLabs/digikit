import fs from 'node:fs';
import path from 'node:path';
import { expect } from 'vitest';
import { parse } from 'smol-toml';
import { liveArtifacts } from '@digi/regression/runtime/artifacts.ts';
import { deviceProfile } from '@digi/regression/runtime/device-data.ts';
import { word } from '@digi/regression/runtime/guest-access.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';
import { planSymbol, prepareRuntimePlan } from '@digi/regression/runtime/runtime-plan.ts';

/**
 * The platform contract (docs/plans/runtime-and-services.md, "Platform adapter"), run against the real platform on each device. The expectation
 * table is `contract.toml` beside this file; the host side of each row is a `FakePlatform` test of the runtime crate (`fake_platform.rs`). The numbers that
 * a device file owns are read from the device file, never written here.
 *
 * Rows checked here: `identity` (the device object's words are the device file's id, name and capabilities), `midi_out` (a reply reaches the
 * host over USB MIDI, so the output works and the TX backlog drained) and `anchors` (the runtime reached its UI hook, so every anchor it links
 * resolved). The other rows name the device test that covers them, or say why none can.
 */
const HERE = path.dirname(new URL(import.meta.url).pathname);
const ROOT = path.join(HERE, '../../../..');
const CHECKS_HERE = ['identity', 'midi_out', 'anchors'];

interface Row {
  id: string;
  device?: string;
  device_pending?: string;
}

const contract = parse(fs.readFileSync(path.join(HERE, 'contract.toml'), 'utf8')) as unknown as { check: Row[] };

runtimeTests('platform contract: identity, MIDI output and anchors on the real platform', async (runtime) => {
  const { session, dk } = runtime;
  await expectRuntimeReady(session); // anchors: the runtime links every anchor it needs, and its UI hook ran

  for (const row of contract.check) {
    expect(row.device !== undefined || row.device_pending !== undefined, `${row.id}: a device check or a reason`).toBe(true);
    if (row.device !== undefined && !CHECKS_HERE.includes(row.device)) {
      expect(fs.existsSync(path.join(ROOT, row.device)), `${row.id}: ${row.device} is a device test file`).toBe(true);
    }
  }

  const device = session.options.device;
  const file = deviceProfile(device);
  const symbols = ['dkrt_device_id', 'dkrt_device_name', 'dkrt_samples_in_store', 'dkrt_sample_path_max'];
  const plan = await prepareRuntimePlan(session, liveArtifacts(session, 'platform-contract'), symbols);
  const read = (name: string) => word(session, planSymbol(plan, name));

  expect(await read('dkrt_device_id'), 'device id').toBe(file.digikit.id);
  const nameWord = file.digikit.name.split('').reduce((a, c) => (a << 8) | c.charCodeAt(0), 0) << (8 * (4 - file.digikit.name.length));
  expect(await read('dkrt_device_name'), 'device name, NUL padded').toBe(nameWord >>> 0);
  expect(await read('dkrt_samples_in_store'), 'samples live in the store exactly when the card capability is "slots"').toBe(
    file.capabilities.card === 'slots' ? 1 : 0,
  );
  expect(await read('dkrt_sample_path_max'), 'sample path limit').toBe(file.digikit.sample_path_max);

  const info = await dk.info();
  expect(info.protocol, 'a reply came back over USB MIDI').toBeGreaterThan(0);
  expect(info.safe_mode, 'a clean boot').toBe(false);
});
