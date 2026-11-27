import assert from 'node:assert/strict';
import { test } from 'vitest';
import { deviceOfInfoPort, deviceOfPort, registry } from '../src/data/device-profiles';

test('firmware INFO ports contain a known device id with an optional version', () => {
  const [device] = registry.devices;
  assert.ok(device);
  assert.equal(deviceOfInfoPort(device.id), device.id);
  assert.equal(deviceOfInfoPort(`${device.id}-1.2`), device.id);
  assert.equal(deviceOfInfoPort('other'), undefined);

  const midiPort = `Elektron ${device.name}`;
  assert.equal(deviceOfPort(midiPort), device.id);
  assert.equal(deviceOfInfoPort(midiPort), undefined);
});
