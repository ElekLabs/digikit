import { afterEach, expect, test, vi } from 'vitest';
import { runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';
import type { EmulatorFlash } from '@digi/workbench-core/firmware/env.ts';
import type { FlashState } from '@digi/workbench-core/firmware/firmware-core.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import type { PreparedFlash, RpcAction, RpcMachine } from '@digi/workbench-core/patch/patchkit-core.ts';
import { sha256Hex } from '@digi/workbench-core/runtime/local-inputs.ts';
import { createEmulatorFlash } from '../src/emulator-flash';

afterEach(() => vi.useRealTimers());

const device = runtimeDeviceIds[0]!;
const base = 'a'.repeat(64);
const container = 'b'.repeat(64);
const syx = 'c'.repeat(64);
const original = {
  session: 7,
  epoch: 11,
  mode: 'free',
  job: null,
  cf: { device, version: '1.17', ready: true },
  firmware: { base_sha256: base, syx_sha256: base },
  flash_boot: null,
};
const booted = {
  ...original,
  epoch: 12,
  firmware: { base_sha256: base, syx_sha256: syx, runtime: { patched: true } },
  flash_boot: { from_epoch: 11, container_sha256: container, syx_sha256: syx },
};

async function fixture() {
  const image = Uint8Array.of(1, 2, 3);
  const p: PreparedFlash = {
    ok: true,
    restore: false,
    device,
    os: '1.17',
    shaStock: base,
    shaImage: await sha256Hex(image),
    containerSha256: container,
    imageId: '0123456789abcdef',
    runtime: '0.5.2',
    paceMs: 2,
    checks: [],
    ends: [image.length],
    image,
  };
  const signal = new AbortController();
  let current: object = original;
  const setStatus = (next: object) => {
    current = next;
  };
  let selected = true;
  const switchTarget = () => {
    selected = false;
  };
  const bytes = (value: number): RpcAction => ({ kind: 'send', bytes: Uint8Array.of(value), timeoutMs: 1_000 });
  let action: RpcAction = bytes(1); // first write
  const cancelled = { kind: 'failed', code: 'cancelled', message: 'Cancelled', taken: 0 } as const;
  const timeout = { kind: 'failed', code: 'timeout', message: 'No acknowledgement', taken: image.length } as const;
  let abortFailure: RpcAction = cancelled;
  const m: RpcMachine = {
    next: () => action,
    taken: () => image.length,
    free: vi.fn(),
    timeout: () => {
      if (action.kind === 'send' && action.bytes[0] === 1) {
        abortFailure = timeout;
        action = bytes(0);
      } else action = timeout;
    },
    cancel: () => {
      action = bytes(0);
      return true;
    },
    confirm: (yes) => {
      action = bytes(yes ? 2 : 0);
    },
    reply: () => {
      if (action.kind === 'send')
        action = action.bytes[0] === 0 ? abortFailure : action.bytes[0] === 1 ? { kind: 'ask' } : { kind: 'reboot' };
      return true;
    },
  };
  const createMachine = vi.fn(async () => m);
  const release = vi.fn();
  const reserveTransport = vi.fn(async () => release);
  const send = vi.fn(async (_msg: number) => {});
  const recv = vi.fn(async () => ({ hex: 'f001f7' }));
  let capturing = false;
  const rpc = {
    call: vi.fn(async (method: string, params?: Record<string, unknown>, session?: number) => {
      if (method === 'status') return current;
      expect(reserveTransport).toHaveBeenCalledOnce();
      expect(release).not.toHaveBeenCalled();
      if (method === 'midi.usb.out') {
        if (capturing) return recv();
        capturing = true;
        return { hex: '' };
      }
      if (method === 'midi.usb.in') {
        expect(capturing).toBe(true);
        expect(session).toBe(original.session);
        expect(params?.epoch).toBe(original.epoch);
        return send(Number.parseInt(String(params?.bytes), 16));
      }
      throw new Error(`Unexpected command ${method}`);
    }),
  };
  const binding: Parameters<EmulatorFlash['probe']>[0] = {
    target: { kind: 'emulator' },
    check: () => {
      if (!selected) throw new Error('The selected target changed.');
    },
    session: async () => ({ reserveTransport }),
  };
  const refreshed = vi.fn(() => expect(release).toHaveBeenCalledOnce());
  const paused = vi.fn(() => false);
  const adapter = createEmulatorFlash(rpc as EmuRpc, createMachine, { paused, refreshAfterFlash: refreshed });
  const states: FlashState[] = [];
  const flash = () => adapter.flash(binding, p, signal.signal, (s) => states.push(s));
  return {
    p,
    signal,
    setStatus,
    switchTarget,
    rpc,
    createMachine,
    send,
    recv,
    paused,
    release,
    reserveTransport,
    binding,
    adapter,
    refreshed,
    states,
    flash,
  };
}

test('waits for a fresh matching boot receipt and ready runtime even when the commit acknowledgement is lost', async () => {
  const f = await fixture();
  vi.useFakeTimers();
  f.send.mockImplementation(async (msg) => {
    if (msg === 2) {
      f.signal.abort();
      f.setStatus({ ...booted, job: { kind: 'boot' }, cf: { ...booted.cf, ready: false } });
      f.recv.mockRejectedValueOnce(new Error('USB is rebooting'));
    }
  });
  const run = f.flash();
  await vi.waitFor(() =>
    expect(f.states).toContainEqual(expect.objectContaining({ phase: 'rebooting', stage: 'returning' })),
  );
  expect(f.refreshed).not.toHaveBeenCalled();
  expect(f.release).not.toHaveBeenCalled();
  f.setStatus(booted);
  await vi.advanceTimersByTimeAsync(250);
  const result = await run;
  expect(result.state.phase).toBe('done');
  expect(f.refreshed).toHaveBeenCalledExactlyOnceWith({ runtime: { patched: true } });
  expect(f.rpc.call.mock.calls.map(([method]) => method)).toContain('midi.usb.out');

  const waitForRestart = await result.beforePackageRestart!();
  const waiting = waitForRestart();
  f.setStatus({ ...booted, epoch: 13 });
  await vi.advanceTimersByTimeAsync(250);
  await waiting;
});

test('rejects a reboot into a different container despite a guest acknowledgement', async () => {
  const f = await fixture();
  f.send.mockImplementation(async (msg) => {
    if (msg === 2) f.setStatus({ ...booted, flash_boot: { ...booted.flash_boot, container_sha256: base } });
  });
  expect((await f.flash()).state).toMatchObject({
    phase: 'failed',
    message: expect.stringMatching(/different firmware container/),
  });
  expect(f.release).toHaveBeenCalledOnce();
  expect(f.refreshed).not.toHaveBeenCalled();
});

test('cancels a pending write with the guest abort while the session remains reserved', async () => {
  const f = await fixture();
  f.recv.mockImplementationOnce(async () => {
    f.signal.abort();
    return { hex: '' };
  });
  expect((await f.flash()).state).toMatchObject({ phase: 'stopped', reason: 'cancelled' });
  expect(f.send.mock.calls.map(([msg]) => msg)).toEqual([1, 0]);
  expect(f.release).toHaveBeenCalledOnce();
  expect(f.refreshed).not.toHaveBeenCalled();
});

test.each(['paused', 'model', 'base', 'digest', 'target'] as const)(
  'refuses %s before the first USB send',
  async (reason) => {
    const f = await fixture();
    if (reason === 'paused') f.setStatus({ ...original, mode: 'paused' });
    if (reason === 'model') f.setStatus({ ...original, cf: { ...original.cf, device: 'other' } });
    if (reason === 'base') f.setStatus({ ...original, firmware: { ...original.firmware, base_sha256: syx } });
    if (reason === 'digest') f.p.containerSha256 = undefined;
    if (reason === 'target')
      f.createMachine.mockImplementationOnce(async () => {
        f.switchTarget();
        return f.createMachine();
      });
    await expect(f.flash()).rejects.toThrow();
    expect(f.send).not.toHaveBeenCalled();
    if (reason === 'target') expect(f.release).toHaveBeenCalledOnce();
    else expect(f.reserveTransport).not.toHaveBeenCalled();
  },
);

test('a failed receive before commit still sends End 0 before releasing the USB session', async () => {
  const f = await fixture();
  f.recv.mockRejectedValueOnce(new Error('Input failed'));
  expect((await f.flash()).state).toMatchObject({ phase: 'stopped', reason: 'error' });
  expect(f.send.mock.calls.map(([msg]) => msg)).toEqual([1, 0]);
  expect(f.release).toHaveBeenCalledOnce();
  expect(f.refreshed).not.toHaveBeenCalled();
});

test('probe blocks while Workbench is paused and finds the stock base after Resume', async () => {
  const f = await fixture();
  f.paused.mockReturnValue(true);
  const result = await f.adapter.probe(f.binding);
  expect(result).toMatchObject({ kind: 'none', why: expect.stringMatching(/Resume/) });
  await expect(f.flash()).rejects.toThrow(/Resume/);
  expect(f.send).not.toHaveBeenCalled();
  f.paused.mockReturnValue(false);
  expect(await f.adapter.probe(f.binding)).toMatchObject({ kind: 'connected', d: { model: device, baseSha256: base } });
});
