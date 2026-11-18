// identifyDevice (dk-session identify.ts) against the fake device: Ping names the model, SoftwareVersion the OS, one short INFO says whether the runtime is there.
import assert from 'node:assert/strict';
import { DkSession, identifyDevice } from '@digi/dk-session';
import { registry } from '@digi/devices/node';
import { FakeDevice, ST } from '@digi/test-support/fakedevice';
import { test } from 'vitest';

const devices = registry();
const session = (dev: FakeDevice) => new DkSession(dev, { timeoutMs: 20, retries: 0 });
const first = devices.devices[0]!;

// The fake's INFO reports one fixed port id for every device, so a matching id here comes from Ping.
test('every registry device is identified by its Ping name, with the runtime', async () => {
  for (const d of devices.devices) {
    const dev = Object.assign(new FakeDevice(), { pingName: d.name, os: '9.9' });
    const who = await identifyDevice(session(dev), devices);
    assert.deepEqual([who.id, who.name, who.os], [d.id, d.name, '9.9'], d.id);
    assert.notEqual(who.runtime, null, d.id);
  }
});

test('a stock device (no runtime) is identified by Ping alone: runtime null, no INFO reply needed', async () => {
  for (const d of devices.devices) {
    const dev = Object.assign(new FakeDevice(), { pingName: d.name, os: '9.9', noRuntime: true });
    const who = await identifyDevice(session(dev), devices);
    assert.deepEqual([who.id, who.runtime, who.info], [d.id, null, null], d.id);
  }
});

test('an unknown Ping name is an error that lists the known names', async () => {
  const dev = Object.assign(new FakeDevice(), { pingName: 'No Such Device', os: '1.2' });
  await assert.rejects(identifyDevice(session(dev), devices), (e: Error) => {
    assert.match(e.message, /No Such Device/);
    assert.match(e.message, /OS 1\.2/);
    for (const d of devices.devices) assert.ok(e.message.includes(d.name), d.name);
    return true;
  });
});

test('a device that does not answer Ping cannot be identified', async () => {
  const dev = Object.assign(new FakeDevice(), { os: '1.2', noRuntime: true });
  await assert.rejects(identifyDevice(session(dev), devices), /does not answer Ping/);
});

test('a runtime whose store is busy still answers INFO after E_BUSY retries', async () => {
  const dev = Object.assign(new FakeDevice(), { pingName: first.name, os: '9.9', infoBusy: 3 });
  const who = await identifyDevice(session(dev), devices);
  assert.equal(who.id, first.id);
  assert.notEqual(who.runtime, null);
});

test('with Ping silent (firmware intro) the id comes from the runtime INFO port', async () => {
  const dev = Object.assign(new FakeDevice(), { pingName: first.name, pingSilent: true });
  const who = await identifyDevice(session(dev), devices);
  // the id is the port the runtime INFO reports
  assert.equal(who.id, (await session(new FakeDevice()).info()).port);
  assert.notEqual(who.runtime, null);
});

test('an INFO error reply is thrown, it is not "no runtime"', async () => {
  const dev = Object.assign(new FakeDevice(), { pingName: first.name, infoStatus: ST.E_STATE });
  await assert.rejects(
    identifyDevice(session(dev), devices),
    (e: unknown) => (e as { kind?: string }).kind !== 'timeout',
  );
});
