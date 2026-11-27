// LogCursor and RuntimeEvents on a fake log with a ring limit and a fake clock: no real time passes.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import {
  LogCursor,
  ModuleLoadRefused,
  RuntimeEvents,
  type Clock,
  type DkLogEvent,
  type WaitRecord,
} from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';

const word = (s: string, i: number) => {
  const part = s.slice(i * 4, i * 4 + 4);
  let n = 0;
  for (let k = 0; k < 4; k++) n = n * 256 + (part.charCodeAt(k) || 0);
  return n;
};

class FakeLog {
  events: DkLogEvent[] = [];
  next = 1;
  pages: number[] = [];
  ring: number;
  pageSize = 4;
  constructor(ring = 1000) {
    this.ring = ring;
  }
  add(kind: keyof typeof LOG_KIND, args: number[] = [], time?: number): DkLogEvent {
    const e = {
      seq: this.next++,
      time: time ?? this.next,
      kind: LOG_KIND[kind],
      flags: 0,
      args: [...args, 0, 0, 0, 0, 0].slice(0, 5),
    };
    this.events.push(e);
    while (this.events.length > this.ring) this.events.shift();
    return e;
  }
  reset() {
    this.events = [];
    this.next = 1;
  }
  async logPage(from: number): Promise<[number, number, DkLogEvent[]]> {
    this.pages.push(from);
    const oldest = this.events[0]?.seq ?? 0;
    const newest = this.events[this.events.length - 1]?.seq ?? 0;
    return [oldest, newest, this.events.filter((e) => e.seq >= from).slice(0, this.pageSize)];
  }
  /** Until true the debug module's commands answer E_PROTO and INFO shows no log sink. */
  debugLoaded = true;
  async info() {
    return { log_sink: this.debugLoaded };
  }
  async logConfig() {
    if (!this.debugLoaded) throw new Error('logcfg: E_PROTO');
    return { level: 1, modules: [0, 0, 0, 0, 0, 0, 0, 0] };
  }
  async setLogConfig() {
    return { level: 1, modules: [], debug: true };
  }
  async modules() {
    return [];
  }
}

class FakeClock implements Clock {
  t = 0;
  async now() {
    return this.t;
  }
  async pause(ms: number) {
    this.t += ms;
  }
}

const make = (log: FakeLog, onWait?: (r: WaitRecord) => void) => {
  const clock = new FakeClock();
  const ev = new RuntimeEvents(log as never, clock, { onWait, tail: 3 });
  return { ev, clock };
};

test('an event after the cursor matches; events before listen() do not', async () => {
  const log = new FakeLog();
  log.add('SAFE');
  const { ev } = make(log);
  await ev.listen();
  await assert.rejects(ev.until('SAFE', undefined, { withinMs: 200 }), /not seen within 200 ms/);
  log.add('SAFE', [3]);
  const e = await ev.until('SAFE');
  assert.equal(e.seq, 2);
});

test('tryUntil gives null when the event does not come and the event when it does', async () => {
  const log = new FakeLog();
  const { ev } = make(log);
  await ev.listen();
  assert.equal(await ev.tryUntil('SAFE', undefined, { withinMs: 200 }), null);
  log.add('SAFE', [3]);
  assert.equal((await ev.tryUntil('SAFE'))?.seq, 1);
});

test('fromBoot sees the early events of the boot', async () => {
  const log = new FakeLog();
  log.add('BOOT_START');
  log.add('UI_FIRST', [0, 0, 2]);
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  assert.equal((await ev.untilBoot()).seq, 1);
});

test('consumed events are not matched twice', async () => {
  const log = new FakeLog();
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  log.add('SAFE');
  log.add('SAFE');
  assert.equal((await ev.until('SAFE')).seq, 1);
  assert.equal((await ev.until('SAFE')).seq, 2);
  await assert.rejects(ev.until('SAFE', undefined, { withinMs: 100 }));
});

test('a new boot is seen by a changed anchor, by BOOT_START after the anchor and by a shorter log', async () => {
  const log = new FakeLog();
  log.add('BOOT_START');
  log.add('SAFE');
  const cur = new LogCursor(log);
  await cur.fromNewest();
  // changed anchor: the log restarted and holds other events at the same numbers
  log.reset();
  log.add('BOOT_START', [], 500);
  log.add('MOD_DSP', [1], 501);
  let r = await cur.read();
  assert.equal(r.newBoot, true);
  assert.deepEqual(
    r.events.map((e) => e.seq),
    [1, 2],
  );
  assert.equal(cur.boots, 1);
  // BOOT_START after an anchor that is still there (the numbers kept going)
  log.add('SAFE');
  log.add('BOOT_START');
  r = await cur.read();
  assert.equal(r.newBoot, true);
  assert.equal(cur.boots, 2);
  // newest below the anchor
  log.add('SAFE');
  log.add('SAFE');
  await cur.read();
  log.reset();
  r = await cur.read();
  assert.equal(r.newBoot, true);
  assert.equal(cur.boots, 3);
  assert.deepEqual(r.events, []);
});

test('a ring that drops exactly the anchor loses nothing and is no new boot', async () => {
  const log = new FakeLog(2);
  log.add('SAFE');
  log.add('SAFE');
  const cur = new LogCursor(log);
  await cur.fromNewest(); // anchor #2
  log.add('SAFE');
  log.add('SAFE'); // the ring keeps #3, #4: the anchor is gone, nothing after it
  const r = await cur.read();
  assert.equal(r.newBoot, false);
  assert.equal(r.lost, 0);
  assert.deepEqual(
    r.events.map((e) => e.seq),
    [3, 4],
  );
  assert.equal(cur.boots, 0);

  const log2 = new FakeLog(2);
  log2.add('SAFE');
  log2.add('SAFE');
  const { ev } = make(log2);
  await ev.listen();
  log2.add('SAFE');
  log2.add('SAFE');
  assert.equal((await ev.until('SAFE')).seq, 3);
  assert.equal(ev.boots, 0);
});

test('a gap is reported as lost; until throws', async () => {
  const log = new FakeLog(5);
  log.add('SAFE');
  const cur = new LogCursor(log);
  await cur.fromNewest();
  for (let i = 0; i < 9; i++) log.add('SAFE');
  const r = await cur.read();
  assert.equal(r.newBoot, false);
  assert.equal(r.lost, 4); // seq 2..5 dropped, 6..10 kept
  assert.deepEqual(
    r.events.map((e) => e.seq),
    [6, 7, 8, 9, 10],
  );

  const log2 = new FakeLog(5);
  log2.add('SAFE');
  const { ev } = make(log2);
  await ev.listen();
  for (let i = 0; i < 9; i++) log2.add('SAFE');
  await assert.rejects(ev.until('BOOT_START'), /lost 4 events/);
});

test('the wait ends after withinMs with the tail of the events seen', async () => {
  const log = new FakeLog();
  const { ev, clock } = make(log);
  await ev.listen({ fromBoot: true });
  for (let i = 0; i < 5; i++) log.add('SAFE', [i]);
  await assert.rejects(ev.until('BOOT_START', undefined, { withinMs: 300 }), (e: Error) => {
    assert.match(e.message, /not seen within 300 ms/);
    assert.match(e.message, /#5 SAFE 4/);
    assert.match(e.message, /#3 SAFE 2/);
    assert.ok(!e.message.includes('#2 SAFE'), 'only the last 3 events');
    return true;
  });
  assert.ok(clock.t >= 300);
});

test('typed waits match on decoded fields', async () => {
  const log = new FakeLog();
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  log.add('UI_FIRST', [0, 0, 1]);
  log.add('UI_FIRST', [0, 0, 4]);
  assert.equal((await ev.untilUiFirst({ loaded: 3 })).seq, 2);
  log.add('MOD_LOAD', [0, 0, word('abcd', 0), word('ef', 0)]);
  log.add('MOD_LOAD', [1, 0, word('usb-', 0), word('fast', 0)]);
  const m = await ev.untilModLoad('usb-fast');
  assert.equal(m.seq, 4);
  log.add('MOD_DSP', [2, 0]);
  log.add('MOD_DSP', [5, 0]);
  assert.equal((await ev.untilModDsp(5)).seq, 6);
  log.add('INPUT_TAKEN', [2, 7, 0xffffffff]);
  assert.equal((await ev.untilInputTaken(7, -1)).seq, 7);
});

test('untilFwReady matches the FW_READY of this boot, also when it was logged before the wait, and fails with the last events when it is missing', async () => {
  const log = new FakeLog();
  log.add('BOOT_START');
  log.add('UI_FIRST', [450, 0, 2]);
  log.add('FW_READY', [450]);
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  const ready = await ev.untilFwReady();
  assert.equal(ready.seq, 3);
  assert.equal(ready.args[0], 450);
  // another boot without it
  log.reset();
  log.add('BOOT_START');
  await assert.rejects(ev.untilFwReady({ withinMs: 100 }), /FW_READY: not seen within 100 ms[\s\S]*BOOT_START/);
});

test('onWait reports success and failure', async () => {
  const log = new FakeLog();
  const records: WaitRecord[] = [];
  const { ev } = make(log, (r) => records.push(r));
  await ev.listen({ fromBoot: true });
  log.add('SAFE');
  await ev.until('SAFE');
  await assert.rejects(ev.until('SAFE', undefined, { withinMs: 100 }));
  assert.equal(records.length, 2);
  assert.deepEqual(
    records.map((r) => [r.kind, r.what, r.ok]),
    [
      ['event', 'SAFE', true],
      ['event', 'SAFE', false],
    ],
  );
  assert.ok(records[1]!.clockMs >= 100);
});

test('two identical boot logs: another boot record in BOOT_START is a new boot, the same one is not', async () => {
  const boot = (seq: number) => {
    const log = new FakeLog();
    log.add('BOOT_START', [1, 2, 3, seq, 0], 0);
    for (let i = 0; i < 6; i++) log.add('SAFE', [i], 10 + i);
    return log;
  };
  // the cursor follows boot record seq 1; the log then holds the same events again, the same numbers and times, with the record seq 5
  const log = boot(1);
  const cur = new LogCursor(log);
  cur.fromBoot();
  await cur.read();
  const again = boot(5);
  log.events = again.events;
  log.next = again.next;
  const r = await cur.read();
  assert.equal(r.newBoot, true);
  assert.equal(cur.boots, 1);
  assert.equal(r.events[0]!.kind, LOG_KIND.BOOT_START);
  // nothing restarted: the same BOOT_START, more events
  log.add('SAFE', [9], 30);
  const same = await cur.read();
  assert.equal(same.newBoot, false);
  assert.deepEqual(
    same.events.map((e) => e.seq),
    [8],
  );
  // a restart with the same boot record and the same events cannot be told from the log (the wait then relies on the RESTART request event): no new boot is claimed
  const identical = boot(5);
  log.events = identical.events;
  log.next = identical.next;
  log.add('SAFE', [9], 30);
  assert.equal((await cur.read()).newBoot, false);
});

test('untilModulesActive waits on MOD_LOAD and MOD_DSP of each module of the selection, and names a refusal with the module the image lacks', async () => {
  const log = new FakeLog();
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  log.add('MOD_LOAD', [0xfffffffe, 0, 0, 0, 0]); // a service file: no index of the selection
  log.add('MOD_LOAD', [0, 0, 0, 0, 0]);
  log.add('MOD_LOAD', [1, 1, 0, 0, 0]); // its DSP part follows
  log.add('MOD_DSP', [1, 0]);
  await ev.untilModulesActive(2); // both are in
  // the DSP part is awaited: result 1 alone is not enough
  const partial = new FakeLog();
  const p = make(partial);
  await p.ev.listen({ fromBoot: true });
  partial.add('MOD_LOAD', [0, 1, 0, 0, 0]);
  await assert.rejects(p.ev.untilModulesActive(1, { withinMs: 300 }), /not seen within/);
  // a refusal for an import: the MOD_NEEDS event (a name of 12 characters in three words) says which core module is not in the image
  const bad = new FakeLog();
  const b = make(bad);
  await b.ev.listen({ fromBoot: true });
  bad.add('MOD_LOAD', [0, 0, 0, 0, 0]);
  bad.add('MOD_LOAD', [1, -7 >>> 0, 0, 0, 0]);
  const word = (t: string) => [...t.padEnd(4, '\0')].reduce((a, c) => a * 256 + c.charCodeAt(0), 0);
  bad.add('MOD_NEEDS', [1, word('samp'), word('le-p'), word('ool')]);
  await assert.rejects(
    b.ev.untilModulesActive(2),
    (e: unknown) => e instanceof ModuleLoadRefused && e.index === 1 && e.result === -7 && e.needs === 'sample-pool',
  );
});

test('untilModulesActive: an E_IMPORT without MOD_NEEDS is final with an empty name, a session without the DSP does not wait for MOD_DSP, and no module is an error naming the debug module', async () => {
  const log = new FakeLog();
  const { ev } = make(log);
  await ev.listen({ fromBoot: true });
  log.add('MOD_LOAD', [0, -7 >>> 0, 0, 0, 0]);
  await assert.rejects(
    ev.untilModulesActive(1),
    (e: unknown) => e instanceof ModuleLoadRefused && e.result === -7 && e.needs === '',
  );
  const noDsp = new FakeLog();
  const n = make(noDsp);
  await n.ev.listen({ fromBoot: true });
  noDsp.add('MOD_LOAD', [0, 1, 0, 0, 0]); // the DSP part never reports without the DSP
  await n.ev.untilModulesActive(1, { dsp: false });
  await assert.rejects(ev.untilModulesActive(0), /debug module file/);
});

test('listen sends no debug-module command until INFO shows the log sink', async () => {
  const log = new FakeLog();
  log.debugLoaded = false;
  const clock = new FakeClock();
  const info = log.info.bind(log);
  let polls = 0;
  log.info = async () => {
    // the module loads (MOD_LOAD) after a few polls
    if (++polls === 4) {
      log.debugLoaded = true;
      log.add('MOD_LOAD', [0, 0]);
    }
    return info();
  };
  const ev = new RuntimeEvents(log as never, clock);
  await ev.listen({ fromBoot: true });
  assert.equal(polls, 4);
  assert.deepEqual(log.pages, []);
});

test('listen fails with the cause when the debug module never loads', async () => {
  const log = new FakeLog();
  log.debugLoaded = false;
  const ev = new RuntimeEvents(log as never, new FakeClock());
  await assert.rejects(ev.listen(), /did not set its log sink/);
});
