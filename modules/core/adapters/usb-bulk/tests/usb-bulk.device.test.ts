import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/card-kit.ts';
import { cardRig, RT } from '@digi/regression/runtime/card-rig.ts';
import { portFile } from '@digi/regression/runtime/device-data.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { advance, rpc } from '@digi/regression/runtime/fx-live.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';
import type { PatchedSession } from '@digi/regression/support.ts';
import type { DkSession } from '@digi/dk-session';
import { LOG_KIND } from '@digi/dk-session/gen_proto';

/**
 * core/usb-bulk (docs/plans/usb-fast.md step 3). The fixture test/usb-echo enables the bulk interface from an I/O job and echoes every buffer back on
 * EP2 IN (tests/fixtures/modules/usb-echo). The emulator does not model the USB controller: `usb.bulk.in` fills the dTDs the adapter has queued on EP2 OUT and
 * calls their completion callbacks at interrupt level, `usb.bulk.out` returns what the adapter queued on EP2 IN, and `connect` stands for the bus reset after
 * the enumeration. What this cannot show (enumeration on a real host, multi-page dTDs, ENDPTFLUSH timing, DMA and cache behaviour) is for the hardware echo.
 */

const ECHO_CMD = 0x2c; // STATUS of test/usb-echo; +1 HOLD
const STOCK = { connect: 0x400044b8, disconnect: 0x4000436e, length: 0x65 };
const PORT_ENTRY = 2;
const ENTRY_WORDS = 10; // 0x28 bytes
const TABLE_ENTRIES = 7; // entry 7 is not a port
const ALIAS = 0x08000000; // DDR_UNCACHED_ALIAS - DDR_CACHED_BASE
/** The stock configuration descriptor of USB MIDI (101 bytes, wTotalLength 0x65 as the firmware writes it at run time; finding 42 section 4). */
const STOCK_CONFIG =
  '09026500020100c0030904000000010100000924010001090001010904010002010300000724010001410006240201010006240202020009240301030102010009240302040101010009050102000200000005250101010905810200020000000525010103';
/** What the adapter serves instead: wTotalLength 0x7c, three interfaces, interface 2 (vendor class, EP2 OUT and IN bulk 512) appended. */
const EXPECTED_CONFIG = (() => {
  const b = Buffer.from(STOCK_CONFIG, 'hex');
  b[2] = 0x7c;
  b[4] = 3;
  return Buffer.concat([
    b,
    Buffer.from('09040200 02ff000000 07050202000200 07058202000200'.replaceAll(' ', ''), 'hex'),
  ]);
})();
const SELECTION = ['modules/usb-echo.dkm'];

const pattern = (n: number, seed: number) => Buffer.from(Uint8Array.from({ length: n }, (_, i) => (i * 7 + (i >> 8) + seed) & 255));

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} core/usb-bulk`, () => {
    const kit = cardRig(device, 'usb-bulk', {
      dsp: 'zero',
      roots: [...RT, 'core/usb-bulk'],
      symbols: ['DKRT_MAILBOX', 'usb_bulk_connect', 'usb_bulk_disconnect', 'usb_bulk_switches', 'usb_bulk_switch_gap_ms', 'usb_bulk_last_area', 'usb_bulk_last_bytes', 'usb_bulk_last_slots', 'usb_bulk_ring_off', 'usb_bulk_tier_cap', 'usb_bulk_free_why', 'usb_bulk_hold_ticks', 'usb_bulk_phy_late', 'usb_bulk_phy_refused', 'usb_bulk_forced_restarts', 'usb_bulk_phy_why', 'usb_bulk_enum_recoveries', 'usb_bulk_audio_state', 'usb_bulk_audio_rx_bytes', 'usb_bulk_audio_rx_frames', 'ua_connect'],
      initial: () => ({ selection: [] }),
      project: true,
      uploads: true,
      relaxFs: true,
    });
    const tableBase = () => {
      const anchors = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((a) => a.name === 'cf.usb.data.port_table');
      expect(anchors.length, 'the port table anchor').toBe(1);
      return anchors[0]!.addr as number;
    };
    const usb = <T>(rt: PatchedSession, run: (dk: DkSession) => Promise<T>) => kit.usb(rt, run);
    const word = async (rt: PatchedSession, addr: number) => {
      const w = await rt.session.client.memPeek(addr);
      expect(w, `guest word at ${addr.toString(16)}`).not.toBeNull();
      return w!;
    };
    /** The port table as the guest holds it: 7 entries of 10 words. */
    const table = async (rt: PatchedSession) => {
      const base = tableBase();
      const words: number[] = [];
      for (let i = 0; i < TABLE_ENTRIES * ENTRY_WORDS; i++) words.push(await word(rt, base + 4 * i));
      return words;
    };
    const entry = (words: number[]) => words.slice(PORT_ENTRY * ENTRY_WORDS, (PORT_ENTRY + 1) * ENTRY_WORDS);
    const bytesAt = async (rt: PatchedSession, addr: number, len: number) => {
      const out = Buffer.alloc((len + 3) & ~3);
      for (let i = 0; i < out.length; i += 4) out.writeUInt32BE(await word(rt, addr + i), i);
      return out.subarray(0, len);
    };
    const echoStatus = async (rt: PatchedSession) => {
      const r = await usb(rt, (dk) => dk.request(ECHO_CMD));
      expect(r.status).toBe(0);
      const b = Buffer.from(r.body);
      return {
        enable: b.readInt32BE(0),
        state: b.readUInt32BE(4),
        steps: b.readUInt32BE(8),
        taken: b.readUInt32BE(12),
        bytesIn: b.readUInt32BE(16),
        bytesOut: b.readUInt32BE(20),
        busy: b.readUInt32BE(24),
        job: b.readInt32BE(28),
        epoch: b.readUInt32BE(32),
      };
    };
    const boot = async (tag: string, selected: boolean) => {
      const rt = await kit.load(tag, { modules: ['test/usb-echo'], selection: selected ? SELECTION : [] });
      if (selected) expect((await bootInfo(rt.session, kit.plan)).results, 'the module loads').toEqual([0]);
      return rt;
    };
    /** Run until the module's job has enabled the interface (usb_bulk_enable returned) and slept. */
    const enabled = async (rt: PatchedSession) => {
      for (let i = 0; i < 100; i++) {
        await advance(rt.session, 100);
        const s = await echoStatus(rt);
        if (s.steps >= 1) return s;
      }
      throw new Error('the module never ran its first step');
    };
    const connect = () => planSymbol(kit.plan, 'usb_bulk_connect');
    const hold = async (rt: PatchedSession, mode: number) =>
      expect((await usb(rt, (dk) => dk.request(ECHO_CMD + 1, Buffer.from([mode])))).status, `hold ${mode}`).toBe(0);
    /** The receive slots of the block allocated last (the ring: 60 slots of 16 KiB when the heap gives a 1 MiB block). */
    const slotsOf = (rt: PatchedSession) => word(rt, planSymbol(kit.plan, 'usb_bulk_last_slots'));
    const poke = (rt: PatchedSession, name: string, value: number) => rpc(rt.session, 'mem.cf.poke', { addr: planSymbol(kit.plan, name), value });
    const full = (state: number) => (state >>> 8) & 0xff;
    const txBusy = (state: number) => (state >>> 16) & 0xff;
    /** What the adapter queued on EP2 IN, until `want` bytes or `limitMs` of emulated time. */
    const collect = async (rt: PatchedSession, want: number, limitMs = 60_000) => {
      const got: Buffer[] = [];
      let have = 0;
      for (let t = 0; t < limitMs && have < want; t += 20) {
        await advance(rt.session, 20);
        const r = await out(rt);
        noSysError(r);
        if (r.bytes > 0) {
          got.push(Buffer.from(r.hex, 'hex'));
          have += r.bytes;
        }
      }
      return Buffer.concat(got);
    };
    /** The bus reset after the enumeration: the first connect callback. */
    const enumerate = async (rt: PatchedSession) => {
      await rpc(rt.session, 'usb.bulk.in', { connect: connect() });
      await advance(rt.session, 50);
    };
    const hostFile = (rt: PatchedSession, name: string, data: Buffer) => {
      const file = path.join(rt.session.runDir, name);
      fs.writeFileSync(file, data);
      return file;
    };
    const out = (rt: PatchedSession) =>
      rpc<{ bytes: number; hex: string; status: { bulk_naks: number; bulk_out_zlps: number; usb_sys_error: string | null } }>(
        rt.session,
        'usb.bulk.out',
        {},
      );
    /** A wedged controller fails the test with its cause instead of a timeout. */
    const noSysError = (r: { status: { usb_sys_error: string | null } }) => {
      if (r.status.usb_sys_error !== null) throw new Error(`emulated USB controller system error: ${r.status.usb_sys_error}`);
    };
    /** Send a host message and collect what comes back until `want` bytes, in emulated time. */
    const echo = async (rt: PatchedSession, data: Buffer, first: boolean, period?: number) => {
      const file = path.join(rt.session.runDir, `bulk-${data.length}.bin`);
      fs.writeFileSync(file, data);
      await rpc(rt.session, 'usb.bulk.in', { file, ...(first ? { connect: connect() } : {}), ...(period ? { period } : {}) });
      const got: Buffer[] = [];
      let have = 0;
      for (let i = 0; i < 4000 && have < data.length; i++) {
        await advance(rt.session, 20);
        const r = await out(rt);
        noSysError(r);
        if (r.bytes > 0) {
          got.push(Buffer.from(r.hex, 'hex'));
          have += r.bytes;
        }
      }
      return Buffer.concat(got);
    };

    test('the module repoints port entry 2 and serves the 124-byte descriptor; unselected, the table is stock', async () => {
      const stockRt = await boot('stock', false);
      await advance(stockRt.session, 500);
      const stock = await table(stockRt);
      expect(entry(stock).slice(0, 2), 'stock callbacks').toEqual([STOCK.connect, STOCK.disconnect]);
      expect(entry(stock)[4], 'stock descriptor length').toBe(STOCK.length);
      expect(entry(stock)[9], 'high speed').toBe(1);

      const rt = await boot('echo', true);
      const before = (await rt.session.client.status()).heartbeat.stall_count;
      const state = await enabled(rt);
      expect(state.enable, 'usb_bulk_enable').toBe(0);
      const t = await table(rt);
      const changed = t.map((w, i) => (w === stock[i] ? -1 : i)).filter((i) => i >= 0);
      const e = PORT_ENTRY * ENTRY_WORDS;
      expect(changed, 'only +0, +4, +0xC and +0x10 of entry 2 changed').toEqual([e, e + 1, e + 3, e + 4]);
      expect(t[e], 'connect callback').toBe(planSymbol(kit.plan, 'usb_bulk_connect'));
      expect(t[e + 1], 'disconnect callback').toBe(planSymbol(kit.plan, 'usb_bulk_disconnect'));
      expect(t[e + 4], 'descriptor length').toBe(EXPECTED_CONFIG.length);
      expect(EXPECTED_CONFIG.length).toBe(124);
      const d = t[e + 3]!;
      expect(d, 'the descriptor lies in the uncached alias of DDR').toBeGreaterThanOrEqual(0x40000000 + ALIAS);
      expect(d % 16, 'aligned').toBe(0);
      expect((await bytesAt(rt, d, EXPECTED_CONFIG.length)).toString('hex'), 'the descriptor bytes').toBe(EXPECTED_CONFIG.toString('hex'));
      expect((await rt.session.client.status()).heartbeat.stall_count, 'no UI stall').toBe(before);
      await rt.session.expectNoFault('enable');
    }, 900_000);

    test('data echoes back identical for every size, through NAK flow control; USB MIDI SysEx still works beside it', async () => {
      const rt = await boot('echo-sizes', true);
      const before = (await rt.session.client.status()).heartbeat.stall_count;
      await enabled(rt);
      let first = true;
      // 1, one packet short of 512, exactly one packet (needs the zero-length packet), several dTDs of 16 KiB, a multiple of 16 KiB (the zero-length packet goes to a dTD of
      // its own), and more than the 60-slot ring holds (the slots are used round the ring and again)
      for (const [i, size] of [1, 511, 512, 513, 1024, 4096, 16384, 65536, 100_000, 262_144, 1_048_576].entries()) {
        const data = pattern(size, i);
        // the host sends a dTD's worth every 8,000 instructions, faster than the module copies one back
        const back = await echo(rt, data, first, 8_000);
        first = false;
        expect(back.length, `echo of ${size} bytes: length`).toBe(size);
        expect(back.equals(data), `echo of ${size} bytes: content`).toBe(true);
      }
      // a slow EP2 IN (a completion every 600,000 instructions): both send slots are in flight, usb_bulk_send says E_BUSY and the job sleeps until the completion wakes it
      const slow = pattern(65_536, 77);
      expect((await echo(rt, slow, false, 600_000)).equals(slow), 'echo with slow send completions').toBe(true);
      const s = await echoStatus(rt);
      expect(s.state & 3, 'enabled and connected').toBe(3);
      expect(s.bytesIn, 'every byte was taken').toBe(s.bytesOut);
      expect(s.busy, 'the send slots ran full at least once').toBeGreaterThan(0);

      // USB MIDI while a large bulk message is in flight
      const big = pattern(262_144, 99);
      await rpc(rt.session, 'usb.bulk.in', { bytes: big.toString('hex') });
      const info = await usb(rt, (dk) => dk.info());
      expect(info, 'INFO over USB MIDI during bulk traffic').toBeTruthy();
      let have = 0;
      const got: Buffer[] = [];
      for (let i = 0; i < 4000 && have < big.length; i++) {
        await advance(rt.session, 20);
        const r = await out(rt);
        if (r.bytes > 0) {
          got.push(Buffer.from(r.hex, 'hex'));
          have += r.bytes;
        }
      }
      expect(Buffer.concat(got).equals(big), 'the bulk message arrived whole').toBe(true);
      expect((await out(rt)).status.usb_sys_error, 'no controller system error').toBeNull();
      expect((await rt.session.client.status()).heartbeat.stall_count, 'no UI stall during transfers').toBe(before);
      await rt.session.expectNoFault('echo');
    }, 1_800_000);

    test('a bus reset cancels the queued transfers, changes the reset count and the interface works again, three times over', async () => {
      const rt = await boot('echo-reset', true);
      await enabled(rt);
      expect((await echo(rt, pattern(2000, 1), true)).length).toBe(2000);
      let epoch = (await echoStatus(rt)).state >>> 24;
      for (let round = 0; round < 3; round++) {
        // the connect callback again, as the firmware gets it after a bus reset or a resume: it cancels what was queued (the stock cancel path of
        // the engine runs our cancel callback for each buffer) and queues the buffers anew
        await rpc(rt.session, 'usb.bulk.in', { connect: connect() });
        await advance(rt.session, 100);
        const s = await echoStatus(rt);
        expect(s.state >>> 24, `reset ${round + 1} changed the reset count`).toBe((epoch + 1) & 0xff);
        epoch = s.state >>> 24;
        expect(s.state & 2, 'connected again').toBe(2);
        const data = pattern(70_000 + round, 10 + round);
        expect((await echo(rt, data, false)).equals(data), `echo after reset ${round + 1}`).toBe(true);
      }
      await rt.session.expectNoFault('reset');
    }, 900_000);

    test('a frame of a multiple of 512 bytes completes only with the zero-length packet (the host\'s duty); the IN side sends 64-byte packets and needs no zero-length packet', async () => {
      const rt = await boot('echo-zlp', true);
      await enabled(rt);
      await enumerate(rt);
      const data = pattern(4096, 5);
      await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, 'noz.bin', data), zlp: false });
      await advance(rt.session, 300);
      expect((await out(rt)).bytes, 'nothing is echoed: the dTD holds eight full packets and is still open').toBe(0);
      const s = await echoStatus(rt);
      expect([s.taken, full(s.state)], 'no buffer was completed').toEqual([0, 0]);
      await rpc(rt.session, 'usb.bulk.in', { zlp: true });
      const back = await collect(rt, data.length);
      expect(back.equals(data), 'the zero-length packet completed the transfer').toBe(true);
      expect((await out(rt)).status.bulk_out_zlps, 'EP2 IN uses 64-byte packets: no zero-length packets').toBe(0);
      await rt.session.expectNoFault('zlp');
    }, 900_000);

    test('a bus reset with a taken buffer and a partly filled dTD, and one with a send in flight, lose no buffer and no slot', async () => {
      const rt = await boot('echo-reset-hold', true);
      await enabled(rt);
      await enumerate(rt);
      const slots = await slotsOf(rt);
      expect(slots, 'the ring: the 1 MiB tier').toBe(60);
      /** All slots are usable: one is taken (held by the job), and the others fill; the last dTD of 16 KiB is NAKed. */
      const capacity = async (tag: string) => {
        await out(rt);
        await hold(rt, 1);
        const data = pattern(slots * 16_384, 20);
        const naks = (await out(rt)).status.bulk_naks;
        await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, `cap-${tag}.bin`, data), period: 20_000 });
        await advance(rt.session, 300);
        expect((await out(rt)).status.bulk_naks, `${tag}: the host is NAKed at the full ring (flow control)`).toBeGreaterThan(naks);
        const s = await echoStatus(rt);
        expect(full(s.state), `${tag}: one slot is held, the others are full: none was lost`).toBe(slots - 1);
        await hold(rt, 0);
        const back = await collect(rt, data.length);
        expect(back.equals(data), `${tag}: and the stream is whole once they are released`).toBe(true);
      };
      // 1. a taken buffer and a partly filled dTD
      await hold(rt, 1);
      await rpc(rt.session, 'usb.bulk.in', { bytes: pattern(3000, 1).toString('hex') });
      await advance(rt.session, 100);
      expect((await echoStatus(rt)).taken, 'the job holds a buffer').toBe(1);
      await rpc(rt.session, 'usb.bulk.in', { bytes: pattern(1024, 2).toString('hex'), zlp: false });
      await advance(rt.session, 100); // the second dTD holds two packets and stays open
      const r0 = (await echoStatus(rt)).state >>> 24;
      await rpc(rt.session, 'usb.bulk.in', { connect: connect() });
      await advance(rt.session, 100);
      let s = await echoStatus(rt);
      expect([s.state >>> 24, s.state & 2], 'reset counted, connected again').toEqual([(r0 + 1) & 0xff, 2]);
      expect(txBusy(s.state)).toBe(0);
      await capacity('after reset with a taken buffer');
      // 2. a send in flight (the job sent one piece of a buffer and holds the rest; the completion is far away)
      await advance(rt.session, 500); // the completions of the last pieces
      await out(rt);
      await hold(rt, 2);
      await rpc(rt.session, 'usb.bulk.in', { bytes: pattern(6000, 3).toString('hex'), period: 5_000_000 });
      await advance(rt.session, 10);
      // no USB MIDI request here: it would queue behind the completion of the send and the emulator would run until that has happened
      const first = await out(rt);
      expect(first.bytes, 'one piece of the buffer is on EP2 IN and its completion is 5,000,000 instructions away').toBe(4096);
      const r1 = s.state >>> 24; // the reset count after the first reset
      await rpc(rt.session, 'usb.bulk.in', { connect: connect() });
      await advance(rt.session, 100);
      s = await echoStatus(rt);
      expect([s.state >>> 24, txBusy(s.state), s.state & 2], 'reset: the slot is free, connected again').toEqual([(r1 + 1) & 0xff, 0, 2]);
      await capacity('after reset with a send in flight');
      await rt.session.expectNoFault('reset with traffic');
    }, 1_800_000);

    test('outside USB MIDI mode enable is refused and the state shows the wrong mode; back in USB MIDI it enables again', async () => {
      const rt = await boot('echo-mode', true);
      await enabled(rt);
      const anchors = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((a) => a.name === 'cf.usb.value.active_port');
      expect(anchors.length, 'the active port anchor').toBe(1);
      const portAddr = anchors[0]!.addr as number;
      const setPort = (value: number) => rpc(rt.session, 'mem.cf.poke', { addr: portAddr, value });
      /** REENABLE, then run until the job has stored a new enable result. */
      const reenable = async (want: number) => {
        expect((await usb(rt, (dk) => dk.request(ECHO_CMD + 2))).status, 'reenable').toBe(0);
        for (let i = 0; i < 100; i++) {
          await advance(rt.session, 100);
          const s = await echoStatus(rt);
          if (s.enable === want) return s;
        }
        throw new Error(`usb_bulk_enable never returned ${want}`);
      };
      try {
        expect(await word(rt, portAddr), 'the firmware is in USB MIDI mode').toBe(2);
        await setPort(5);
        const refused = await reenable(-53);
        expect(refused.state & 0x4, 'wrong USB mode').toBe(0x4);
        await setPort(2);
        const again = await reenable(0);
        expect(again.state & 0x1, 'enabled').toBe(0x1);
        expect(again.state & 0x4, 'USB MIDI mode').toBe(0);
        await enumerate(rt);
        const data = pattern(4096, 31);
        expect((await echo(rt, data, false)).equals(data), 'echo after the mode returned to USB MIDI').toBe(true);
        await rt.session.expectNoFault('usb mode');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    /** The take-over tests' helpers: the firmware's active port (a menu-style change pokes it), the stored mode byte, the echo module's TAKEOVER command. */
    const takeoverKit = (rt: PatchedSession) => {
      const anchor = (name: string) => {
        const a = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((x) => x.name === name);
        expect(a.length, name).toBe(1);
        return a[0]!.addr as number;
      };
      const portAddr = anchor('cf.usb.value.active_port');
      const modeAddr = anchor('cf.usb.value.config_mode');
      /** A menu-style change of the mode: the active port and the firmware's pointer to its table entry (the word below the port), as the mode switch leaves them. */
      const setPort = async (value: number) => {
        await rpc(rt.session, 'mem.cf.poke', { addr: portAddr - 4, value: tableBase() + value * 0x28 });
        await rpc(rt.session, 'mem.cf.poke', { addr: portAddr, value });
      };
      const port = () => word(rt, portAddr);
      /** The USB CONFIG setting byte (the most significant byte of its aligned word). */
      const mode = async () => (await word(rt, modeAddr)) >>> 24;
      const takeover = async (op: number) => {
        const r = await usb(rt, (dk) => dk.request(ECHO_CMD + 3, Buffer.from([op])));
        expect(r.status, `takeover op ${op}`).toBe(0);
        return Buffer.from(r.body).readInt32BE(0);
      };
      const taken = async () => ((await echoStatus(rt)).state & 0x8) !== 0;
      const run = async (ms: number) => {
        for (let t = 0; t < ms; t += 1000) await advance(rt.session, Math.min(1000, ms - t));
      };
      return { setPort, port, mode, takeover, taken, run };
    };

    test('take-over: USB AUDIO/MIDI to USB MIDI from the tick and back; the stored mode is never written; the watchdog, the renew and the menu rule', async () => {
      const rt = await boot('echo-takeover', true);
      await enabled(rt);
      const { setPort, port, mode, takeover, taken, run } = takeoverKit(rt);
      const stored = await mode();
      try {
        // Overbridge: refused, nothing changes
        await setPort(6);
        expect(await takeover(0), 'refused in Overbridge').toBe(-53);
        await run(300);
        expect([await port(), await taken()], 'Overbridge untouched').toEqual([6, false]);

        // USB AUDIO/MIDI: the switch runs from the tick, not in the command
        await setPort(5);
        expect(await takeover(0), 'takeover').toBe(0);
        expect(await port(), 'the command itself does not switch').toBe(5);
        expect(await taken(), 'the state shows the take-over at once').toBe(true);
        await run(4500); // the boot's enable counts as a switch: the guard (2 s) runs first
        expect(await port(), 'switched to USB MIDI').toBe(2);
        expect(await taken(), 'still taken').toBe(true);
        expect(await mode(), 'the stored mode is unchanged').toBe(stored);
        expect(await takeover(1), 'giveback').toBe(0);
        await run(4500); // the guard after the take-over switch
        expect([await port(), await taken()], 'back in USB AUDIO/MIDI').toEqual([5, false]);
        expect(await mode(), 'the stored mode is unchanged after the give-back').toBe(stored);

        // the watchdog: 30 s without activity, a renew in between keeps it
        expect(await takeover(0)).toBe(0);
        await run(20_000);
        expect(await takeover(0), 'renew').toBe(0);
        await run(20_000);
        expect([await port(), await taken()], 'renewed: still taken after 40 s').toEqual([2, true]);
        await run(12_000);
        expect([await port(), await taken()], 'no activity for 30 s: given back').toEqual([5, false]);

        // the user's choice wins: a menu change drops the take-over and nothing switches back
        expect(await takeover(0)).toBe(0);
        await run(4500); // after the watchdog's give-back switch
        expect(await port()).toBe(2);
        await setPort(5);
        await run(1000);
        expect([await port(), await taken()], 'menu change: dropped, no switch back').toEqual([5, false]);

        // already in USB MIDI: no switch
        await setPort(2);
        expect(await takeover(0), 'takeover in USB MIDI').toBe(0);
        await run(500);
        expect([await port(), await taken()], 'USB MIDI: taken, no switch').toEqual([2, true]);
        expect(await takeover(1)).toBe(0);
        await run(500);
        expect([await port(), await taken()], 'given back without a switch').toEqual([2, false]);
        expect(await mode(), 'the stored mode is unchanged at the end').toBe(stored);
        await rt.session.expectNoFault('takeover');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    test('the buffers exist only for a session: freed at the give-back (EP2 stays up, the host is NAKed, nothing lands in the freed block), allocated again by enable without a re-enumeration; a take-over from USB AUDIO/MIDI allocates before its one switch and frees after the give-back', async () => {
      const LIVE = 0x10;
      const rt = await boot('echo-free', true);
      await enabled(rt);
      const { setPort, port, takeover, run } = takeoverKit(rt);
      const state = async () => (await echoStatus(rt)).state;
      const switches = () => word(rt, planSymbol(kit.plan, 'usb_bulk_switches'));
      const block = async () => ({
        addr: await word(rt, planSymbol(kit.plan, 'usb_bulk_last_area')),
        bytes: await word(rt, planSymbol(kit.plan, 'usb_bulk_last_bytes')),
      });
      /** REENABLE, then run until the job has stored an enable result of 0 (a refused one is retried: the guard after a switch). */
      const reenable = async () => {
        for (let i = 0; i < 100; i++) {
          expect((await usb(rt, (dk) => dk.request(ECHO_CMD + 2))).status, 'reenable').toBe(0);
          await advance(rt.session, 100);
          const s = await echoStatus(rt);
          if (s.enable === 0) return s;
        }
        throw new Error('usb_bulk_enable never returned 0');
      };
      try {
        expect((await state()) & LIVE, 'the buffers are allocated by the enable in USB MIDI mode').toBe(LIVE);
        const area = await block();
        const slots = await slotsOf(rt);
        expect([area.addr % 4096, slots, area.bytes], 'the block: 4 KiB aligned, the ring of 60 slots of 16 KiB and 2 send slots of 16 KiB').toEqual([0, 60, 60 * 16_384 + 32_768]);
        const first = pattern(70_000, 41);
        expect((await echo(rt, first, true)).equals(first), 'transfer 1').toBe(true);

        // a session in USB MIDI mode: take over (no switch), give back: the tick frees the buffers; EP2 stays enabled
        // the job holds a receive buffer while the session ends: the free waits for it (CPU-owned memory), then happens once it is released
        const freeWhy = async () => word(rt, planSymbol(kit.plan, 'usb_bulk_free_why'));
        const taken0 = (await echoStatus(rt)).taken;
        await hold(rt, 1);
        await rpc(rt.session, 'usb.bulk.in', { bytes: pattern(3000, 45).toString('hex') });
        await advance(rt.session, 100);
        expect((await echoStatus(rt)).taken, 'the job holds a buffer').toBe(taken0 + 1);
        expect(await takeover(0), 'takeover in USB MIDI').toBe(0);
        expect(await takeover(1), 'giveback').toBe(0);
        await run(1500);
        expect([(await state()) & LIVE, await freeWhy()], 'not freed while a buffer is held (reason 1)').toEqual([LIVE, 1]);
        await hold(rt, 0);
        expect((await collect(rt, 3000)).length, 'the held buffer is echoed once released').toBe(3000);
        await run(1500);
        const freed = await state();
        expect(await freeWhy(), 'freed').toBe(0);
        const why = (await freeWhy()).toString(16);
        expect([freed & LIVE, freed & 2, freed & 1], `freed (refused for: 0x${why}); EP2 still up (connected); still prepared`).toEqual([0, 2, 1]);

        // the host sends while there are no buffers: nothing lands in the freed block (poisoned here), the host is NAKed
        const poison = 0xa5a5a5a5;
        const spots = [0, 16_384, slots * 16_384, slots * 16_384 + 16_384].map((o) => area.addr + ALIAS + o);
        for (const a of spots) for (let k = 0; k < 8; k++) await rpc(rt.session, 'mem.cf.poke', { addr: a + 4 * k, value: poison });
        const held = pattern(5000, 44);
        await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, 'while-freed.bin', held) });
        await advance(rt.session, 500);
        expect((await out(rt)).bytes, 'nothing is echoed while there are no buffers').toBe(0);
        expect(full((await state())), 'no buffer was filled').toBe(0);
        for (const a of spots) for (let k = 0; k < 8; k++) expect(await word(rt, a + 4 * k), `the freed block at ${(a + 4 * k).toString(16)} is untouched`).toBe(poison);

        // enable again: allocated again, the waiting data arrives whole, and enable made no switch of its own (EP2 was up: no re-enumeration)
        const s0 = await switches();
        const again = await reenable();
        expect((await state()) & LIVE, 'allocated again').toBe(LIVE);
        expect(again.enable).toBe(0);
        expect(await switches(), 'no stop and restart for a session in USB MIDI mode').toBe(s0);
        expect((await collect(rt, held.length)).equals(held), 'the data the host had waiting arrives whole').toBe(true);
        const second = pattern(70_001, 42);
        expect((await echo(rt, second, false)).equals(second), 'transfer 2 is the new data, whole: no stale DMA of the first session').toBe(true);

        // a take-over from USB AUDIO/MIDI: the buffers exist before the switch connects port 2 (one switch, one enumeration), and are gone after the give-back
        expect(await takeover(0)).toBe(0);
        expect(await takeover(1)).toBe(0);
        await run(1500);
        expect((await state()) & LIVE, 'freed after the second give-back').toBe(0);
        await setPort(5);
        await run(4500); // the guard after the enable's restart
        const s1 = await switches();
        expect(await takeover(0), 'takeover from USB AUDIO/MIDI').toBe(0);
        await run(4500);
        expect(await port(), 'switched to USB MIDI').toBe(2);
        expect(await switches(), 'the take-over made exactly one switch').toBe(s1 + 1);
        expect((await state()) & LIVE, 'the tick allocated before the switch').toBe(LIVE);
        await enumerate(rt);
        const third = pattern(40_000, 43);
        expect((await echo(rt, third, false)).equals(third), 'transfer 3 after a take-over').toBe(true);
        expect(await takeover(1), 'giveback').toBe(0);
        await run(4500);
        expect(await port(), 'back in USB AUDIO/MIDI').toBe(5);
        expect((await state()) & LIVE, 'freed again; EP2 untouched in USB AUDIO/MIDI').toBe(0);
        await rt.session.expectNoFault('buffers freed and allocated');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    test('the receive ring: the switch and the allocation tiers size the block (60, 28 or 8 slots; a failure of every tier changes nothing), every size echoes whole through the wrap, and a full ring NAKs the host and loses nothing', async () => {
      const LIVE = 0x10;
      const NOMEM = 0x20;
      const rt = await boot('echo-ring', true);
      await enabled(rt);
      const { takeover, run } = takeoverKit(rt);
      const state = async () => (await echoStatus(rt)).state;
      const bytesOf = (slots: number) => slots * 16_384 + 32_768;
      /** Free the buffers (a take-over in USB MIDI mode and its give-back), then REENABLE until usb_bulk_enable has an answer other than a retry. */
      const cycle = async () => {
        expect(await takeover(0)).toBe(0);
        expect(await takeover(1)).toBe(0);
        await run(1500);
        expect((await state()) & LIVE, 'freed').toBe(0);
      };
      const reenable = async () => {
        for (let i = 0; i < 100; i++) {
          expect((await usb(rt, (dk) => dk.request(ECHO_CMD + 2))).status, 'reenable').toBe(0);
          await advance(rt.session, 100);
          const s = await echoStatus(rt);
          if (s.enable !== -53) return s;
        }
        throw new Error('usb_bulk_enable kept answering E_STATE');
      };
      const alloc = async (tag: string, slots: number) => {
        const s = await reenable();
        expect([s.enable, (s.state & LIVE) !== 0], `${tag}: enabled, live`).toEqual([0, true]);
        expect([await slotsOf(rt), await word(rt, planSymbol(kit.plan, 'usb_bulk_last_bytes'))], `${tag}: the tier`).toEqual([slots, bytesOf(slots)]);
        const data = pattern(slots * 16_384 + 70_001, slots); // more than the ring holds: the slots are used round the ring and again
        expect((await echo(rt, data, false)).equals(data), `${tag}: ${data.length} bytes echo whole through the wrap`).toBe(true);
      };
      expect(await slotsOf(rt), 'the default: the 1 MiB tier').toBe(60);
      expect(await word(rt, planSymbol(kit.plan, 'usb_bulk_ring_off')), 'the switch is on by default').toBe(0);
      const first = pattern(1_000_000, 3);
      expect((await echo(rt, first, true, 20_000)).equals(first), 'the ring: a megabyte echoes whole').toBe(true);

      // switch off: the old geometry (8 slots), allocated by the next session
      await cycle();
      await poke(rt, 'usb_bulk_ring_off', 1);
      await alloc('switch off', 8);
      // on again; the heap cannot give the 1 MiB block: 28 slots; cannot give 512 KiB either: the old 8
      await cycle();
      await poke(rt, 'usb_bulk_ring_off', 0);
      await poke(rt, 'usb_bulk_tier_cap', 28);
      await alloc('1 MiB block refused', 28);
      await cycle();
      await poke(rt, 'usb_bulk_tier_cap', 8);
      await alloc('1 MiB and 512 KiB blocks refused', 8);
      // every tier refused: E_NOMEM, ST_NOMEM, nothing allocated, nothing queued, the host NAKed; the next success clears it
      await cycle();
      await poke(rt, 'usb_bulk_tier_cap', 1);
      const refused = await reenable();
      expect([refused.enable, refused.state & LIVE, refused.state & NOMEM, full(refused.state)], 'no tier: E_NOMEM, not live, ST_NOMEM').toEqual([-8, 0, NOMEM, 0]);
      await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, 'no-buffers.bin', pattern(5000, 1)) });
      await advance(rt.session, 300);
      expect(full(await state()), 'nothing lands without buffers').toBe(0);
      await poke(rt, 'usb_bulk_tier_cap', 0);
      const back = await reenable();
      expect([back.enable, back.state & LIVE, back.state & NOMEM], 'allocated again, ST_NOMEM cleared').toEqual([0, LIVE, 0]);
      expect([await slotsOf(rt), (await collect(rt, 5000)).length], 'the ring again, and the data that waited arrives').toEqual([60, 5000]);
      await rt.session.expectNoFault('receive ring');
    }, 1_800_000);

    test('take-over hold: the PHY is released while the controller is stopped and put back before the restart, the restart waits 60 ticks, one switch per take-over, and a lost restart is forced', async () => {
      const rt = await boot('echo-hold', true);
      await enabled(rt);
      const { setPort, port, takeover, run } = takeoverKit(rt);
      const anchor = (name: string) => {
        const a = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((x) => x.name === name);
        expect(a.length, name).toBe(1);
        return a[0]!.addr as number;
      };
      const delayAddr = anchor('cf.usb.value.restart_delay');
      const sym = (name: string) => word(rt, planSymbol(kit.plan, name));
      type PhyEvent = { w?: number[]; r?: number[]; rs?: number };
      const phy = () => rpc<{ events: PhyEvent[]; regs: string }>(rt.session, 'usb.phy', {});
      /** Advance in 20 ms steps and collect the PHY events (without the firmware's own register 0 reads) with the emulated time they were seen at, until `done` or `limitMs`. */
      let clock = 0; // emulated ms advanced by `watch`
      const watch = async (done: (events: { t: number; e: PhyEvent }[]) => boolean, limitMs: number) => {
        const seen: { t: number; e: PhyEvent }[] = [];
        for (let t = 0; t < limitMs && !done(seen); t += 20) {
          await advance(rt.session, 20);
          clock += 20;
          for (const e of (await phy()).events) seen.push({ t: clock, e });
        }
        return seen;
      };
      const FC = 4;
      try {
        await setPort(5);
        await run(4500); // the boot's enable is a switch: let its guard pass
        const before = await phy();
        const fcBefore = parseInt(before.regs.slice(FC * 2, FC * 2 + 2), 16);
        const s0 = await sym('usb_bulk_switches');
        expect(await takeover(0), 'takeover').toBe(0);
        const writes = (seen: { t: number; e: PhyEvent }[]) => seen.filter((x) => x.e.w !== undefined);
        const first = await watch((seen) => writes(seen).length >= 1, 8000);
        expect(writes(first).map((x) => x.e.w), 'the first PHY write releases the line: function control, non-driving, no termination').toEqual([[FC, 0x49]]);
        expect(await port(), 'the switch ran').toBe(2);
        const left = await word(rt, delayAddr);
        expect(left, 'the firmware restart counter was raised from 30 to 60').toBeGreaterThan(30);
        expect(left).toBeLessThanOrEqual(60);
        expect(await sym('usb_bulk_hold_ticks')).toBe(60);
        const detachedAt = writes(first)[0]!.t;
        const rest = await watch((seen) => seen.some((x) => x.e.rs === 1) || (writes(seen).length >= 1 && seen.length >= 2), 5000);
        const restore = writes(rest);
        expect(restore.map((x) => x.e.w), 'then the saved function control is written back').toEqual([[FC, fcBefore]]);
        const heldMs = restore[0]!.t - detachedAt;
        expect(heldMs, 'the line is released for about 2 s (57 ticks of 33 ms)').toBeGreaterThanOrEqual(1700);
        expect(heldMs).toBeLessThanOrEqual(2400);
        // the order after the detach write: the restore, then RS = 1 (when the model's firmware starts the controller at all)
        const order = rest.map((x) => (x.e.w !== undefined ? 'w' : x.e.rs === 1 ? 'rs1' : 'rs0'));
        expect(order.indexOf('w'), 'the restore comes before any RS = 1').toBeGreaterThanOrEqual(0);
        if (order.includes('rs1')) expect(order.indexOf('rs1'), 'RS is set after the restore').toBeGreaterThan(order.indexOf('w'));
        expect(await sym('usb_bulk_switches'), 'one switch for the take-over').toBe(s0 + 1);
        expect(await sym('usb_bulk_forced_restarts'), 'no forced restart on the normal path').toBe(0);
        expect(await sym('usb_bulk_phy_refused'), 'no refused ULPI access').toBe(0);
        expect(await sym('usb_bulk_phy_late'), 'the PHY was restored in time').toBe(0);

        // a lost restart: the counter is gone, the controller stopped, VBUS present. The adapter restarts it itself, once.
        await run(4500);
        expect(await takeover(1), 'giveback').toBe(0);
        await run(6000);
        expect(await port(), 'back in USB AUDIO/MIDI').toBe(5);
        expect(await takeover(0)).toBe(0);
        for (let t = 0; t < 8000 && (await port()) !== 2; t += 20) await watch(() => false, 20);
        expect(await port(), 'the take-over switched').toBe(2);
        await rpc(rt.session, 'mem.cf.poke', { addr: delayAddr, value: 0 }); // the firmware's tick no longer restarts
        await rpc(rt.session, 'usb.phy', { vbus: true }); // OTGSC.BSV: VBUS present
        const forced = await watch((seen) => seen.some((x) => x.e.rs === 1), 8000);
        expect(forced.some((x) => x.e.rs === 1), 'the adapter forced the restart').toBe(true);
        const resetAt = forced.findIndex((x) => x.e.w !== undefined && x.e.w[0] === FC && x.e.w[1] === 0x61);
        expect(resetAt, 'after a PHY reset').toBeGreaterThanOrEqual(0);
        expect(resetAt, 'which comes before RS = 1').toBeLessThan(forced.findIndex((x) => x.e.rs === 1));
        expect(await sym('usb_bulk_forced_restarts'), 'once').toBe(1);
        await rt.session.expectNoFault('takeover hold');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    test('USB_SW events: take-over and release log the switch timeline up to the bus reset, and ENUM_LATE when no host addresses the device (no host model in the emulator)', async () => {
      const USB_SW = LOG_KIND.USB_SW;
      const PHASE = { REQ: 1, FW_SWITCH: 3, HOLD_BEGIN: 5, PHY_PARK: 6, RS_UP: 8, CONNECT_CB: 9, ADDRESSED: 10, ENUM_LATE: 13, RECOVERY: 14, GIVEUP: 15, DONE: 16, PHY_BUSY: 19 };
      const rt = await kit.load('echo-sw-events', { modules: ['test/usb-echo', 'digikit/debug'], selection: [...SELECTION, 'modules/debug.dkm'] });
      await enabled(rt);
      const { setPort, port, takeover, run } = takeoverKit(rt);
      const phaseOf = (e: { args: readonly number[] }) => e.args[0]! & 0xff;
      const dirOf = (e: { args: readonly number[] }) => (e.args[0]! >>> 12) & 1;
      // the daemon is paused between steps: read the log over the stepping `usb` helper and advance the emulator ourselves
      let cursor = (await usb(rt, (dk) => dk.logPage(0xffffffff)))[1] + 1;
      const seen: number[] = [];
      /** Wait for the next USB_SW event of `phase` and `dir`; the events before it are skipped, so the calls check the order. */
      const next = async (phase: number, dir: number, emulatedMs = 12_000) => {
        for (let t = 0; t <= emulatedMs; t += 20) {
          for (;;) {
            const [, newest, page] = await usb(rt, (dk) => dk.logPage(cursor));
            const fresh = page.filter((e) => e.seq >= cursor);
            for (const e of fresh) {
              cursor = e.seq + 1;
              if (e.kind !== USB_SW) continue;
              seen.push(phaseOf(e) | (dirOf(e) << 12));
              if (phaseOf(e) === phase && dirOf(e) === dir) return e;
            }
            if (fresh.length === 0 || cursor > newest) break;
          }
          await advance(rt.session, 20);
        }
        throw new Error(`no USB_SW phase ${phase} dir ${dir} in ${emulatedMs} emulated ms; last seen (phase | dir << 12): ${seen.slice(-8).join(', ')}`);
      };
      try {
        await setPort(5);
        await run(4500); // the boot's enable is a switch: let its guard pass
        await rpc(rt.session, 'usb.phy', { vbus: true }); // OTGSC.BSV: VBUS present, so the firmware restarts the controller
        expect(await takeover(0), 'takeover').toBe(0);
        const req = await next(PHASE.REQ, 0);
        expect((req.args[0]! >>> 8) & 0xf, 'REQ names the target port').toBe(2);
        await next(PHASE.FW_SWITCH, 0);
        await next(PHASE.HOLD_BEGIN, 0);
        // the firmware's own restart sets RS; the connect callback comes after RS_UP (take-over only)
        const up = await next(PHASE.RS_UP, 0);
        await rpc(rt.session, 'usb.bulk.in', { connect: connect() });
        await next(PHASE.CONNECT_CB, 0);
        const late = await next(PHASE.ENUM_LATE, 0, 6000);
        expect(late.args[1]! - up.args[1]!, 'ENUM_LATE comes 3.0 to 3.5 s after RS_UP').toBeGreaterThanOrEqual(3000);
        expect(late.args[1]! - up.args[1]!).toBeLessThanOrEqual(3500);

        // a failed take-over gets no controller reset (it would wipe the dTD pool of the bulk endpoint): GIVEUP directly
        await next(PHASE.GIVEUP, 0, 2000);
        expect(await word(rt, planSymbol(kit.plan, 'usb_bulk_enum_recoveries')), 'no recovery for a take-over').toBe(0);

        // the release: the port 5 connect is the firmware's, so no CONNECT_CB
        await run(4500);
        expect(await takeover(1), 'giveback').toBe(0);
        const rel = await next(PHASE.REQ, 1);
        expect((rel.args[0]! >>> 8) & 0xf, 'REQ names the original port').toBe(5);
        await next(PHASE.FW_SWITCH, 1);
        await next(PHASE.HOLD_BEGIN, 1);
        await next(PHASE.RS_UP, 1);
        await next(PHASE.ENUM_LATE, 1, 6000);
        // the watchdog: two recoveries (the firmware's stop with controller reset; the emulator models USBCMD.RST), each followed by RS_UP and a new ENUM_LATE, then GIVEUP. The emulator's PHY viewport completes at once, so PHY_BUSY cannot be produced here.
        for (const n of [1, 2]) {
          const rec = await next(PHASE.RECOVERY, 1, 4000);
          expect((rec.args[0]! >>> 13) & 7, `recovery try ${n}`).toBe(n);
          await next(PHASE.RS_UP, 1, 6000);
          await next(PHASE.ENUM_LATE, 1, 6000);
        }
        await next(PHASE.GIVEUP, 1, 2000);
        expect(await word(rt, planSymbol(kit.plan, 'usb_bulk_enum_recoveries')), 'two recoveries').toBe(2);
        expect(
          seen.filter((s) => (s & 0xff) === PHASE.PHY_BUSY),
          'the PHY never refused in the emulator',
        ).toEqual([]);
        expect(await port(), 'back in USB AUDIO/MIDI').toBe(5);
        // the PHY park is compiled out (UB_ULPI_DETACH 0): its events never occur
        expect(
          seen.filter((s) => (s & 0xff) === PHASE.PHY_PARK),
          'no PHY_PARK',
        ).toEqual([]);

        for (const phase of [PHASE.ADDRESSED, PHASE.DONE]) {
          expect(
            seen.filter((s) => (s & 0xff) === phase),
            `no host: phase ${phase} never logged`,
          ).toEqual([]);
        }
        await rt.session.expectNoFault('USB_SW');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    test('USB AUDIO/MIDI bulk OUT probe: entry 5 serves the 0x158-byte descriptor with interface 5 appended, and the host\'s bulk data on EP2 OUT is counted', async () => {
      const rt = await boot('echo-audio-probe', true);
      await enabled(rt);
      const { setPort, port } = takeoverKit(rt);
      const sym = (name: string) => word(rt, planSymbol(kit.plan, name));
      try {
        await setPort(5);
        expect(await port(), 'USB AUDIO/MIDI is the active port').toBe(5);
        expect(await sym('usb_bulk_audio_state'), 'ua_prepare ran and accepted the stock descriptor').toBe(1);
        const e5 = tableBase() + 5 * 0x28;
        const length = await word(rt, e5 + 0x10);
        expect(length, 'entry 5 descriptor length').toBe(0x158);
        const d = await word(rt, e5 + 0xc);
        const desc = await bytesAt(rt, d, length);
        expect(desc.readUInt16LE(2), 'wTotalLength').toBe(0x0158);
        expect(desc[4], 'bNumInterfaces').toBe(6);
        expect(desc.subarray(0x148).toString('hex'), 'interface 5: vendor class, EP2 OUT bulk 512').toBe('09040500' + '01ff000000' + '07050202000200');

        // delivery: usb.bulk.in is generic (the QH 0x80000100 list and ENDPTCTRL2 of the model), independent of the active port. `connect` stands for the
        // SET_CONFIGURATION that runs the port 5 connect callback (ua_connect), which primes the receive ring; the emulator has no host to send it.
        // If the model lacks something for port 5 here (e.g. it NAKs because ENDPTCTRL2 RX is not set), the counters stay at 0 and `bulk_naks` grows.
        const rx = async () => ({ bytes: await sym('usb_bulk_audio_rx_bytes'), frames: await sym('usb_bulk_audio_rx_frames') });
        const r0 = await rx();
        const size = 64 * 1024;
        await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, 'audio-probe-0.bin', pattern(size, 1)), connect: planSymbol(kit.plan, 'ua_connect') });
        await advance(rt.session, 300);
        for (let i = 1; i < 3; i++) {
          await rpc(rt.session, 'usb.bulk.in', { file: hostFile(rt, `audio-probe-${i}.bin`, pattern(size, 1 + i)) });
          await advance(rt.session, 300);
        }
        const status = (await out(rt)).status;
        noSysError({ status });
        const r1 = await rx();
        expect(r1.bytes - r0.bytes, 'rx_bytes grew by the three transfers').toBe(3 * size);
        expect(r1.frames - r0.frames, 'rx_frames grew by at least one per transfer').toBeGreaterThanOrEqual(3);
        await rt.session.expectNoFault('audio probe');
      } finally {
        await setPort(2);
      }
    }, 900_000);

    /** The PHY-log helpers of the hold tests below: the restart counter, symbols, the log of `usb.phy`, and a watch that advances in steps and returns the events seen. */
    const phyKit = (rt: PatchedSession) => {
      const a = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((x) => x.name === 'cf.usb.value.restart_delay');
      expect(a.length, 'cf.usb.value.restart_delay').toBe(1);
      const delayAddr = a[0]!.addr as number;
      const sym = (name: string) => word(rt, planSymbol(kit.plan, name));
      type Ev = { w?: number[]; r?: number[]; rs?: number };
      let clock = 0;
      const watch = async (ms: number, done: (seen: { t: number; e: Ev }[]) => boolean = () => false) => {
        const seen: { t: number; e: Ev }[] = [];
        for (let t = 0; t < ms && !done(seen); t += 20) {
          await advance(rt.session, 20);
          clock += 20;
          for (const e of (await rpc<{ events: Ev[] }>(rt.session, 'usb.phy', {})).events) seen.push({ t: clock, e });
        }
        return seen;
      };
      /** Advance until the active port is `want`; the events of that time are dropped. */
      const untilPort = async (port: () => Promise<number>, want: number) => {
        for (let t = 0; t < 8000 && (await port()) !== want; t += 20) await watch(20);
        expect(await port(), `the active port became ${want}`).toBe(want);
      };
      return { delayAddr, sym, watch, untilPort };
    };

    test('take-over hold, late path: a controller started before the write-back is read and restored when it still holds the released value', async () => {
      const rt = await boot('echo-hold-late', true);
      await enabled(rt);
      const { setPort, port, takeover, run } = takeoverKit(rt);
      const { sym, watch } = phyKit(rt);
      try {
        await setPort(5);
        await run(4500);
        await rpc(rt.session, 'usb.phy', {});
        expect(await takeover(0), 'takeover').toBe(0);
        const detach = await watch(8000, (seen) => seen.some((x) => x.e.w !== undefined));
        expect(detach.filter((x) => x.e.w !== undefined).map((x) => x.e.w), 'released').toEqual([[4, 0x49]]);
        // something starts the controller long before the write-back is due (the counter still has 50 ticks to run)
        await rpc(rt.session, 'usb.phy', { run: true });
        const seen = await watch(1000, (s2) => s2.some((x) => x.e.w !== undefined));
        expect(seen.find((x) => x.e.w)?.e.w, 'after RS = 1 the value that was read before the release is written back').toEqual([4, 0]);
        expect(await sym('usb_bulk_phy_late'), 'counted as late').toBe(1);
        expect(await sym('usb_bulk_phy_refused')).toBe(0);
        await rt.session.expectNoFault('late write-back');
      } finally {
        await setPort(2);
      }
      void port;
    }, 1_800_000);

    test('take-over recovery: no forced restart on stale state (no VBUS, then plug-in; a menu mode change) and the window starts at the plug-in', async () => {
      const rt = await boot('echo-stale', true);
      await enabled(rt);
      const { setPort, port, takeover, run } = takeoverKit(rt);
      const { sym, watch, untilPort } = phyKit(rt);
      const vbus = (on: boolean) => rpc(rt.session, 'usb.phy', { vbus: on });
      try {
        await setPort(5);
        await run(4500);
        // a switch with no VBUS: the firmware cannot start the controller; the adapter must not either, however long it waits
        expect(await takeover(0)).toBe(0);
        await untilPort(port, 2);
        await watch(6000);
        expect(await sym('usb_bulk_forced_restarts'), 'no VBUS: nothing forced').toBe(0);
        // the plug-in: a new window of 4 s starts now
        await vbus(true);
        await watch(3000);
        expect(await sym('usb_bulk_forced_restarts'), 'inside the window after the plug-in').toBe(0);
        const forced = await watch(3000, (seen) => seen.some((x) => x.e.rs === 1));
        expect(forced.some((x) => x.e.rs === 1), 'after the window the restart is forced').toBe(true);
        expect(await sym('usb_bulk_forced_restarts')).toBe(1);

        // a menu mode change disarms: a switch with no VBUS, the user picks another mode, VBUS comes: nothing is forced
        await vbus(false);
        await rpc(rt.session, 'usb.phy', { run: false });
        await setPort(5);
        await run(4500);
        expect(await takeover(0)).toBe(0);
        await untilPort(port, 2);
        await setPort(5); // the menu
        await watch(1000);
        await vbus(true);
        await watch(7000);
        expect(await sym('usb_bulk_forced_restarts'), 'a menu change disarmed the recovery').toBe(1);
        await rt.session.expectNoFault('stale recovery');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);

    test('switch guard: one firmware mode switch at a time (enable waits, release then take-over and a give-back racing the tick make two spaced switches, never overlapping ones)', async () => {
      const GUARD_MS = 500; // the adapter's floor on its own clock; the firmware's restart (about 1 s) is waited for through its delay counter
      const rt = await boot('echo-guard', true);
      await enabled(rt);
      const { setPort, port, mode, takeover } = takeoverKit(rt);
      /** Emulated time that this test has advanced, ms. */
      let now = 0;
      const adv = async (ms: number) => {
        await advance(rt.session, ms);
        now += ms;
      };
      const run = async (ms: number) => {
        for (let t = 0; t < ms; t += 1000) await adv(Math.min(1000, ms - t));
      };
      const switches = () => word(rt, planSymbol(kit.plan, 'usb_bulk_switches'));
      const gap = () => word(rt, planSymbol(kit.plan, 'usb_bulk_switch_gap_ms'));
      const reenableOnce = async () => {
        expect((await usb(rt, (dk) => dk.request(ECHO_CMD + 2))).status, 'reenable').toBe(0);
        await adv(100);
        return (await echoStatus(rt)).enable;
      };
      /** Advance in small steps until the active port is `want`, then return at once with the time (the switch has just run). */
      const until = async (want: number, limitMs = 8000) => {
        for (let t = 0; t < limitMs; t += 5) {
          if ((await port()) === want) return now;
          await adv(5);
        }
        throw new Error(`the active port never became ${want}`);
      };
      const stored = await mode();
      try {
        await setPort(5);
        const s0 = await switches();
        expect(await takeover(0), 'takeover').toBe(0);
        await until(2);
        await until(2);
        expect(await switches(), 'the take-over made one switch').toBe(s0 + 1);

        // enable inside the guard: retry (E_STATE), no stop or restart of its own; after the guard it succeeds
        expect(await reenableOnce(), 'enable inside the guard').toBe(-53);
        expect(await reenableOnce(), 'enable inside the guard, again').toBe(-53);
        expect(await switches(), 'a refused enable made no switch').toBe(s0 + 1);
        let enableRc = -53;
        for (let i = 0; i < 100 && enableRc !== 0; i++) enableRc = await reenableOnce();
        expect(enableRc, 'enable succeeds once the guard has passed').toBe(0);
        expect(await switches(), 'the adapter was enabled already: no second restart').toBe(s0 + 1);
        await run(1000);

        // release, then a take-over after the give-back switch ran: two switches at least the guard apart
        const s1 = await switches();
        expect(await takeover(1), 'giveback').toBe(0);
        const back = await until(5);
        expect(await switches(), 'the give-back switched').toBe(s1 + 1);
        expect(await takeover(0), 'take-over right after the give-back').toBe(0);
        await adv(100);
        expect([await port(), await switches()], 'the second switch waits for the restart').toEqual([5, s1 + 1]);
        const again = await until(2);
        expect(await switches(), 'then the take-over switches').toBe(s1 + 2);
        expect(again - back, 'two switches at least the firmware restart delay (about 1 s) apart').toBeGreaterThanOrEqual(900);
        expect(await gap(), 'and the adapter floor on its own clock').toBeGreaterThanOrEqual(GUARD_MS);

        // a give-back requested at the moment the tick switched (the closest the test can get): one switch for each request, spaced
        expect(await takeover(1)).toBe(0);
        await run(6000);
        expect(await port(), 'back in USB AUDIO/MIDI').toBe(5);
        const s2 = await switches();
        expect(await takeover(0)).toBe(0);
        await until(2);
        const took = now;
        expect(await takeover(1), 'giveback right after the switch').toBe(0);
        await adv(300);
        expect([await port(), await switches()], 'the give-back waits').toEqual([2, s2 + 1]);
        const gave = await until(5);
        expect(await switches(), 'one give-back switch, after the restart').toBe(s2 + 2);
        expect(gave - took, 'spaced by the firmware restart delay').toBeGreaterThanOrEqual(900);
        expect(await gap(), 'spaced by the floor').toBeGreaterThanOrEqual(GUARD_MS);
        await run(3000);
        expect(await switches(), 'nothing else switched').toBe(s2 + 2);
        expect(await mode(), 'the stored mode is unchanged').toBe(stored);
        await rt.session.expectNoFault('switch guard');
      } finally {
        await setPort(2);
      }
    }, 1_800_000);
  });
}
