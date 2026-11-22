import { expect, test, vi } from 'vitest';
import { createRemoteAudio, matchedInput } from '../src/remote-audio';
import { createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';

const model = registry.devices[0]!.name;
const port = 'Elektron ' + model;
const input = { id: 'usb-input', name: model };
const other = { id: 'other', name: 'Studio interface' };

function setup() {
  const host = {
    inputs: vi.fn(async () => [other, input]),
    start: vi.fn(async (_id: string) => 'Headphones'),
    stop: vi.fn(async () => {}),
    status: vi.fn(async (): Promise<string | null> => null),
  };
  return { host, audio: createRemoteAudio(host, async () => [port]) };
}

test('auto matches a complete model uniquely on both sides and never guesses from the default input', () => {
  for (const device of registry.devices) {
    const name = device.name;
    expect(matchedInput('Elektron ' + name, [{ id: 'a', name }], ['Elektron ' + name])?.id).toBe('a');
  }
  expect(matchedInput(port, [input, { ...input, id: 'duplicate' }], [port])).toBeUndefined();
  expect(matchedInput(port, [input], [port, port + ' 2'])).toBeUndefined();
  expect(matchedInput(port, [input, { ...other, name: `Line (${port})` }], [port])).toBeUndefined();
  expect(matchedInput(port, [other], [port])).toBeUndefined();
  expect(matchedInput('IAC', [{ id: 'mic', name: 'IAC' }], ['IAC'])).toBeUndefined();
});

test('manual choice and Off belong to the selected port; leaving releases the stream', async () => {
  const { audio, host } = setup();
  await audio.select(port);
  expect(host.start).toHaveBeenCalledWith(input.id);
  expect(audio.state().output).toBe('Headphones');
  await audio.select('IAC');
  expect(host.stop).toHaveBeenCalledOnce();
  expect(audio.state().output).toBeUndefined();
  await audio.choose(other.id);
  expect(host.start).toHaveBeenLastCalledWith(other.id);
  await audio.choose('off');
  await audio.select();
  await audio.select('IAC');
  expect(audio.state().choice).toBe('off');
  expect(host.start).toHaveBeenCalledTimes(2);
});

test('a switch during native start releases the old stream before another input starts', async () => {
  const { audio, host } = setup();
  let finish!: (output: string) => void;
  let started!: () => void;
  const opening = new Promise<void>((resolve) => {
    started = resolve;
  });
  host.start.mockImplementationOnce(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve;
        started();
      }),
  );
  const first = audio.select(port);
  await opening;
  const leaving = audio.select();
  const next = audio.select(port);
  finish('Old output');
  await Promise.all([first, leaving, next]);
  expect(host.stop.mock.invocationCallOrder[0]).toBeLessThan(host.start.mock.invocationCallOrder[1]!);
  expect(audio.state().output).toBe('Headphones');
});

test('unplug clears playback, shows a recoverable error and rescan reopens the input', async () => {
  const { audio, host } = setup();
  await audio.select(port);
  host.status.mockResolvedValueOnce('Input disconnected');
  await audio.check();
  expect(host.stop).toHaveBeenCalledOnce();
  expect(audio.state().error).toBe('Input disconnected');
  expect(audio.state().output).toBeUndefined();
  await audio.refresh();
  expect(audio.state().output).toBe('Headphones');
});

test('an unavailable manual selection never falls back to another input', async () => {
  const { audio, host } = setup();
  await audio.select('IAC');
  await audio.choose(other.id);
  host.inputs.mockResolvedValue([input]);
  await audio.refresh();
  expect(host.start).toHaveBeenCalledTimes(1);
  expect(audio.state().error).toMatch(/unavailable/);
});

test('a failed close is reported to the target switch and a later release can succeed', async () => {
  const { audio, host } = setup();
  await audio.select(port);
  host.stop.mockRejectedValueOnce(new Error('Endpoint busy'));
  await expect(audio.release()).rejects.toThrow('Endpoint busy');
  await audio.release();
  expect(host.stop).toHaveBeenCalledTimes(2);
});

test('the target stays on MIDI until the monitor has released both streams', async () => {
  const { target } = createFakeTransferApp();
  const { audio, host } = setup();
  await target.selectTarget({ kind: 'midi', port });
  const source = target.bindSession();
  await audio.select(port);
  source.beforeLeave(audio.release);
  let closed!: () => void;
  let stopping!: () => void;
  const pending = new Promise<void>((resolve) => {
    stopping = resolve;
  });
  host.stop.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        closed = resolve;
        stopping();
      }),
  );
  const leaving = target.selectTarget({ kind: 'emulator' });
  await pending;
  expect(target.target()).toBe(source.target);
  closed();
  await leaving;
  expect(target.target().kind).toBe('emulator');
});
