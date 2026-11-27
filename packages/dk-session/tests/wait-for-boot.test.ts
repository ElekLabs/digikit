// waitForBoot after restart(): with the debug module the log shows the new boot (BOOT_START without the RESTART request, then FW_READY); without it INFO has no LOG
// capability and the device answering again after a silent spell does. The time is the session clock: a fake clock moves only when the wait pauses.
import assert from 'node:assert/strict';
import { test, vi } from 'vitest';
import { DkError, DkSession, FW_READY_GRACE_MS, decode8in7, emulatedProbe, encode8in7 } from '@digi/dk-session';
import { CAP, CMD as GCMD, LOG_KIND, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const concat = (...p: Uint8Array[]) => Uint8Array.from(Buffer.concat(p.map((x) => Buffer.from(x))));
const PREFIX = [0xf0, 0x7d, 0x64, 0x6b, 1];
const reply = (cmd: number, seq: number, status: number, body: number[] = []) =>
  concat(
    Uint8Array.from(PREFIX),
    Uint8Array.of(cmd | 0x40, seq),
    encode8in7(Uint8Array.of(status & 255, ...body)),
    Uint8Array.of(0xf7),
  );
const be32 = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];

/** A clock that moves only when paused; `onPause` lets a test flip the device between rounds; `pauses` counts them. */
const fakeClock = (onPause: (n: number) => void = () => {}) => {
  let t = 0;
  const clock = {
    pauses: 0,
    now: async () => t,
    pause: async (ms: number) => {
      t += ms;
      onPause(++clock.pauses);
    },
  };
  return clock;
};

type BootEvent = { kind: number; a0?: number };

/** A device whose INFO has, while `debug` is set, the LOG capability and a log. */
class Device {
  debug = false;
  silent = false;
  rebooted = false;
  /** The log of the new boot (a test sets a shorter one for an older runtime). */
  bootLog: BootEvent[] = [{ kind: LOG_KIND.BOOT_START }, { kind: LOG_KIND.UI_FIRST }, { kind: LOG_KIND.FW_READY }];
  queue: Uint8Array[] = [];
  send(msg: Uint8Array) {
    if (this.silent) return;
    const cmd = msg[5]!;
    const seq = msg[6]!;
    if (cmd === GCMD.INFO) {
      const caps = (1 << CAP.SERVICE_CMDS) | (1 << CAP.MODULE_SECTIONS) | (this.debug ? 1 << CAP.LOG : 0);
      const body = [0, 1, 0, 5, ...Buffer.from('dt2\0'), ...be32(1000), ...be32(32768), 0, ...be32(0), ...be32(0)];
      body.push(...be32(0x000500), ...be32(caps), 0, 0, 0, 0, 0, 0, 0, 0);
      return void this.queue.push(reply(cmd, seq, 0, body));
    }
    if (cmd === GCMD.RESTART) return void this.queue.push(reply(cmd, seq, 0));
    if (cmd === SERVICE_CMD.LOG) {
      if (!this.debug) return void this.queue.push(reply(cmd, seq, -60));
      const body = decode8in7(msg.subarray(7, msg.length - 1));
      const from = new DataView(body.buffer, body.byteOffset).getUint32(0);
      // the log: a RESTART request of the old boot, or the events of the new one
      const log: BootEvent[] = this.rebooted ? this.bootLog : [{ kind: LOG_KIND.UPLOAD, a0: GCMD.RESTART }];
      const evs = log.map((e, i) => ({ seq: i + 1, ...e })).filter((e) => e.seq >= from);
      const bytes = evs.flatMap((e) => [
        ...be32(e.seq),
        ...be32(0),
        e.kind >> 8,
        e.kind & 255,
        0,
        0,
        ...be32(e.a0 ?? 0),
        ...be32(0),
        ...be32(0),
        ...be32(0),
        ...be32(0),
      ]);
      return void this.queue.push(reply(cmd, seq, 0, [...be32(1), ...be32(log.length), evs.length, ...bytes]));
    }
    this.queue.push(reply(cmd, seq, -60));
  }
  async recv() {
    return this.queue.shift() ?? null;
  }
}

test('without the debug module an answer after a silent spell is the new boot', async () => {
  const dev = new Device();
  const clock = fakeClock((n) => {
    dev.silent = n === 2; // the old boot answers, the device resets (silent), the new boot answers
  });
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock });
  await s.restart();
  await s.waitForBoot(5000);
  assert.equal(clock.pauses, 3);
});

test('a dropped INFO probe does not prove a fresh boot when boot evidence is required', async () => {
  const dev = new Device();
  const clock = fakeClock((n) => {
    dev.silent = n === 2;
  });
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock });
  await s.restart();
  await assert.rejects(s.waitForBoot(2500, { requireBootEvidence: true }), /no new boot.*without a boot log/);
});

test('with the debug module the log shows the new boot: BOOT_START, no RESTART request, then FW_READY', async () => {
  const dev = new Device();
  dev.debug = true;
  const clock = fakeClock((n) => {
    dev.rebooted = n >= 3; // the old boot's log holds the RESTART request; after more rounds the log is the new boot's
  });
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock });
  await s.restart();
  await s.waitForBoot(5000, { requireBootEvidence: true });
  assert.equal(clock.pauses, 3);
});

test('after the first round the log is read from the cursor, not from the start', async () => {
  const dev = new Device();
  dev.debug = true;
  const clock = fakeClock((n) => {
    dev.rebooted = n >= 4;
  });
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock });
  await s.restart();
  const froms: number[] = [];
  const page = s.logPage.bind(s);
  s.logPage = (from, quick) => {
    froms.push(from);
    return page(from, quick);
  };
  await s.waitForBoot(5000);
  assert.equal(froms[0], 0);
  // each later round reads the boot's identity (the log head, one page from 0) and then the events after the cursor
  assert.ok(
    froms.slice(2, -1).every((f, i) => f === (i % 2 === 0 ? 0 : 1)),
    `rounds after the first ask the log head, then the last event: ${froms}`,
  );
  assert.ok(froms.filter((f) => f === 1).length >= 2, `the events are read from the cursor: ${froms}`);
  assert.equal(froms[froms.length - 1], 0, 'a new boot is read from its start');
});

test('FW_READY ends the wait; a boot that only logged UI_FIRST is accepted after the grace window; a stuck boot times out', async () => {
  // a new boot with BOOT_START, UI_FIRST and FW_READY: returns in the round that read them
  const full = new Device();
  full.debug = true;
  full.rebooted = true;
  const fullClock = fakeClock();
  await new DkSession(full, { timeoutMs: 200, retries: 0, clock: fullClock }).waitForBoot(5000);
  assert.equal(fullClock.pauses, 0);
  // FW_READY appears 5 rounds after UI_FIRST, inside the grace window: the wait ends with it, not at the round after UI_FIRST
  const late = new Device();
  late.debug = true;
  late.rebooted = true;
  late.bootLog = [{ kind: LOG_KIND.BOOT_START }, { kind: LOG_KIND.UI_FIRST }];
  const lateClock = fakeClock((n) => {
    if (n === 5) late.bootLog.push({ kind: LOG_KIND.FW_READY });
  });
  await new DkSession(late, { timeoutMs: 200, retries: 0, clock: lateClock }).waitForBoot(5000, { pollMs: 100 });
  assert.equal(lateClock.pauses, 5);
  // an older runtime logs no FW_READY: the wait ends after the grace window, not before
  const old = new Device();
  old.debug = true;
  old.rebooted = true;
  old.bootLog = [{ kind: LOG_KIND.BOOT_START }, { kind: LOG_KIND.UI_FIRST }];
  const oldClock = fakeClock();
  await new DkSession(old, { timeoutMs: 200, retries: 0, clock: oldClock }).waitForBoot(5000, { pollMs: 100 });
  assert.equal(oldClock.pauses, FW_READY_GRACE_MS / 100);
  // a boot that never reaches its UI loop
  const stuck = new Device();
  stuck.debug = true;
  stuck.rebooted = true;
  stuck.bootLog = [{ kind: LOG_KIND.BOOT_START }];
  await assert.rejects(
    new DkSession(stuck, { timeoutMs: 200, retries: 0, clock: fakeClock() }).waitForBoot(1000),
    /no new boot within 1 s .*the new boot has not reached its UI loop/,
  );
});

test('until boot ends at BOOT_START, whatever the UI loop does', async () => {
  const dev = new Device();
  dev.debug = true;
  dev.rebooted = true;
  dev.bootLog = [{ kind: LOG_KIND.BOOT_START }];
  const clock = fakeClock();
  await new DkSession(dev, { timeoutMs: 200, retries: 0, clock }).waitForBoot(5000, { until: 'boot' });
  assert.equal(clock.pauses, 0);
});

test('a refused restart in the log still ends the wait with refused', async () => {
  class RefusedDevice extends Device {
    send(msg: Uint8Array) {
      if (msg[5] !== SERVICE_CMD.LOG) return super.send(msg);
      const ev = [
        ...be32(1),
        ...be32(0),
        LOG_KIND.UPLOAD >> 8,
        LOG_KIND.UPLOAD & 255,
        0,
        1, // flags: the reply status is in argument 1
        ...be32(GCMD.RESTART),
        ...be32(5),
        ...be32(0),
        ...be32(0),
        ...be32(0),
      ];
      this.queue.push(reply(msg[5]!, msg[6]!, 0, [...be32(1), ...be32(1), 1, ...ev]));
    }
  }
  const dev = new RefusedDevice();
  dev.debug = true;
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock: fakeClock() });
  await s.restart();
  await assert.rejects(s.waitForBoot(5000), (e: unknown) => e instanceof DkError && e.kind === 'refused');
});

/** A device whose replies take `delayMs` of wall time; a receive that waits less gets nothing, as on a real transport. */
class SlowDevice extends Device {
  delayMs = 0;
  async recv(waitMs?: number) {
    if (this.queue.length === 0) return null;
    const want = this.delayMs;
    if (waitMs !== undefined && want > waitMs) {
      await new Promise((r) => setTimeout(r, waitMs));
      return null;
    }
    await new Promise((r) => setTimeout(r, want));
    return this.queue.shift() ?? null;
  }
}

test('a probe timeout longer than the default lets a slow host show the boot', async () => {
  const dev = new SlowDevice();
  dev.debug = true;
  dev.rebooted = true;
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock: fakeClock() });
  await s.restart();
  dev.delayMs = 1300; // longer than the 1 s quick probe
  await s.waitForBoot(20000, { probeTimeoutMs: 5000 });
});

/** A device that loses the first request (sent while it restarts); a receive ends at its wait or when `signal` fires. */
class LostDevice extends Device {
  lose = true;
  send(msg: Uint8Array) {
    if (this.lose) return void (this.lose = false);
    return super.send(msg);
  }
  async recv(waitMs?: number, signal?: AbortSignal) {
    if (this.queue.length > 0) return this.queue.shift()!;
    await new Promise<void>((r) => {
      const t = setTimeout(r, waitMs ?? 1000);
      signal?.addEventListener('abort', () => (clearTimeout(t), r()), { once: true });
    });
    return this.queue.shift() ?? null;
  }
}

test('a probe lost during the restart ends with the caller signal, and the next probe sees the new boot', async () => {
  const dev = new LostDevice();
  dev.debug = true;
  dev.rebooted = true;
  let probes = 0;
  const s = new DkSession(dev, {
    timeoutMs: 60_000,
    retries: 0,
    clock: fakeClock(),
    // the harness's emulated bound: the first probe ends after 40 ms of the fake clock, far below the wall guard
    emulatedProbe: async (run) => {
      probes++;
      const ctl = new AbortController();
      const timer = setTimeout(() => ctl.abort(), 40);
      try {
        return await run({ signal: ctl.signal, ms: 60_000 });
      } finally {
        clearTimeout(timer);
      }
    },
  });
  const started = Date.now();
  await s.waitForBoot(20_000);
  assert.equal(probes, 4, 'the lost info probe, then info, the boot identity and the log read of the second try');
  assert.ok(Date.now() - started < 5000, 'a lost probe did not wait for the wall guard');
});

test('a lost first request during the reset costs the probe bound, not the session timeout', async () => {
  const dev = new LostDevice();
  dev.debug = true;
  dev.rebooted = true;
  let t = 0;
  const clock = { now: async () => (t += 1000), pause: async () => {} }; // a status stream: 1 s of emulated time per reading
  const s = new DkSession(dev, { timeoutMs: 60_000, retries: 0, clock, emulatedProbe: emulatedProbe(clock) });
  const started = Date.now();
  await s.waitForBoot(20_000);
  assert.ok(Date.now() - started < 5000, 'one lost probe, 2 s of emulated time at 100 wall ms per reading, not 60 s');
});

test('a slow reply still succeeds while the signal has not fired', async () => {
  const dev = new SlowDevice();
  dev.debug = true;
  dev.rebooted = true;
  const ctl = new AbortController();
  const s = new DkSession(dev, {
    timeoutMs: 200,
    retries: 0,
    clock: fakeClock(),
    emulatedProbe: (run) => run({ signal: ctl.signal, ms: 60_000 }),
  });
  await s.restart();
  dev.delayMs = 1300; // wall-slow, as on a loaded host; the harness clock (emulated) has not reached its bound
  await s.waitForBoot(20_000);
  assert.equal(ctl.signal.aborted, false);
});

test('a cancelled read ends the wait', async () => {
  const dev = new Device();
  const s = new DkSession(dev, { timeoutMs: 50, retries: 0, clock: fakeClock() });
  s.info = async () => {
    throw new DkError({ error: 'cancelled', kind: 'cancelled' });
  };
  await assert.rejects(s.waitForBoot(5000), (e: unknown) => e instanceof DkError && e.kind === 'cancelled');
});

test('the wait does not time out while the boot is reported', async () => {
  const clock = fakeClock();
  const s = new DkSession(new Device(), { timeoutMs: 50, retries: 0, clock });
  let reads = 0;
  s.info = async () => {
    if (++reads <= 5) throw new Error('no answer'); // the port is silent while the controller boots
    return { caps: null } as Awaited<ReturnType<DkSession['info']>>;
  };
  // 5 rounds of 500 ms are 2500 ms, more than the 1000 ms timeout; the reported boot keeps the wait alive
  await s.waitForBoot(1000, { stillBooting: () => reads < 5, pollMs: 500 });
  assert.equal(reads, 6);
  assert.ok((await clock.now()) > 1000);
});

test('an aborted signal ends the wait', async () => {
  const dev = new Device();
  const stop = new AbortController();
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock: fakeClock(() => stop.abort()) });
  await s.restart();
  await assert.rejects(
    s.waitForBoot(5000, { signal: stop.signal }),
    (e: unknown) => e instanceof DkError && e.kind === 'cancelled',
  );
});

test('a device that never restarts and has no log times out', async () => {
  const dev = new Device();
  const s = new DkSession(dev, { timeoutMs: 50, retries: 0, clock: fakeClock() });
  await s.restart();
  await assert.rejects(s.waitForBoot(30), /no new boot/);
});

test('waitForRestart returns MODULES once the boot is ready, and retries a failing MODULES on the session clock', async () => {
  const dev = new Device();
  dev.debug = true;
  dev.rebooted = true;
  const clock = fakeClock();
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock });
  s.modules = vi
    .fn()
    .mockRejectedValueOnce(new Error('lost'))
    .mockRejectedValueOnce(new Error('lost'))
    .mockResolvedValue([{ name: 'a' }] as never);
  const started = Date.now();
  const modules = await s.waitForRestart();
  assert.deepEqual(
    modules.map((m) => m.name),
    ['a'],
  );
  assert.ok(clock.pauses >= 2);
  assert.ok(Date.now() - started < 1000, 'the retries ran on the fake clock');
  const never = new DkSession(dev, { timeoutMs: 200, retries: 0, clock: fakeClock() });
  never.modules = vi.fn().mockRejectedValue(new Error('lost'));
  await assert.rejects(
    never.waitForRestart({ loadedTimeoutMs: 500 }),
    /MODULES after the restart: not seen within 500 ms/,
  );
});

test('waitForRestart: a lost MODULES reply costs the probe bound plus one retry, not the session timeout', async () => {
  class DropModulesDevice extends Device {
    drop = true;
    send(msg: Uint8Array) {
      if (msg[5] === GCMD.MODULES) {
        if (this.drop) return void (this.drop = false);
        return void this.queue.push(
          reply(
            msg[5]!,
            msg[6]!,
            0,
            Array.from({ length: 18 }, () => 0),
          ),
        );
      }
      return super.send(msg);
    }
    async recv(waitMs?: number) {
      if (this.queue.length > 0) return this.queue.shift()!;
      await new Promise((r) => setTimeout(r, waitMs ?? 1000));
      return this.queue.shift() ?? null;
    }
  }
  const dev = new DropModulesDevice();
  dev.debug = true;
  dev.rebooted = true;
  const s = new DkSession(dev, { timeoutMs: 60_000, retries: 2, clock: fakeClock() });
  const started = Date.now();
  const modules = await s.waitForRestart({ probeTimeoutMs: 300 });
  assert.deepEqual(modules, []);
  assert.ok(dev.drop === false);
  assert.ok(Date.now() - started < 5000, 'the lost reply cost one probe of 300 ms, not 60 s');
});

test('waitForRestart does not retry a cancelled MODULES', async () => {
  const dev = new Device();
  dev.debug = true;
  dev.rebooted = true;
  const s = new DkSession(dev, { timeoutMs: 200, retries: 0, clock: fakeClock() });
  const modules = vi.fn().mockRejectedValue(new DkError({ error: 'cancelled', kind: 'cancelled' }));
  s.modules = modules;
  await assert.rejects(s.waitForRestart(), (e: unknown) => e instanceof DkError && e.kind === 'cancelled');
  assert.equal(modules.mock.calls.length, 1);
});
