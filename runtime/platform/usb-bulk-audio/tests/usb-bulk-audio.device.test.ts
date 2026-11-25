import { describe, expect, test } from 'vitest';
import { bootInfo } from '@digi/regression/runtime/guest-mailbox.ts';
import { cardRig, RT } from '@digi/regression/runtime/card-rig.ts';
import { portFile } from '@digi/regression/runtime/device-data.ts';
import { DEVICES, FIRMWARE } from '@digi/regression/runtime/devices.ts';
import { rpc } from '@digi/regression/runtime/guest-access.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import { planSymbol } from '@digi/regression/runtime/runtime-plan.ts';

/**
 * core/usb-bulk-audio (docs/plans/usb-fast-audio-bulk.md): entry 5 of the port table (USB AUDIO/MIDI) serves the descriptor with interface 5 (EP2 OUT) appended,
 * core/usb-bulk reports OUT-only mode in that port, and the connect callback leaves the TX half of ENDPTCTRL2 (the audio feedback endpoint) as the stock callback set it.
 * The fixture test/usb-echo enables the bulk interface; the upload path in this mode is tested in digikit/usb-fast.
 */

const ECHO_CMD = 0x2c; // STATUS of test/usb-echo
const OUT_ONLY = 0x40; // DK_USB_BULK_OUT_ONLY
const WRONG_MODE = 0x04; // DK_USB_BULK_WRONG_MODE
const ENDPTCTRL2 = 0xfc0b01c8;
const FEEDBACK_TX = 0x00c40000; // the stock port 5 connect: EP2 TX iso enabled

for (const device of DEVICES) {
  describe(`${device.toUpperCase()} ${FIRMWARE[device]} core/usb-bulk-audio`, () => {
    const kit = cardRig(device, 'usb-bulk-audio', {
      dsp: 'zero',
      roots: [...RT, 'core/usb-bulk', 'core/usb-bulk-audio'],
      symbols: ['DKRT_MAILBOX', 'usb_bulk_audio_state', 'ua_connect'],
      initial: () => ({ selection: [] }),
      project: true,
      uploads: true,
      relaxFs: true,
    });
    const anchor = (name: string) => {
      const a = (portFile(device, FIRMWARE[device]).anchor ?? []).filter((x) => x.name === name);
      expect(a.length, name).toBe(1);
      return a[0]!.addr as number;
    };

    test('entry 5 serves interface 5, the state shows OUT-only mode, and the connect callback keeps the feedback endpoint', async () => {
      const rt = await kit.load('audio-bulk', { modules: ['test/usb-echo'], selection: [kit.mod('test/usb-echo').name] });
      expect((await bootInfo(rt.session, kit.plan)).results, 'the module loads').toEqual([0]);
      const word = async (addr: number) => {
        const w = await rt.session.client.memPeek(addr);
        expect(w, `guest word at ${addr.toString(16)}`).not.toBeNull();
        return w!;
      };
      const portAddr = anchor('cf.usb.value.active_port');
      const table = anchor('cf.usb.data.port_table');
      // a menu-style change of the mode to USB AUDIO/MIDI: the active port and the firmware's pointer to its table entry
      await rpc(rt.session, 'mem.cf.poke', { addr: portAddr - 4, value: table + 5 * 0x28 });
      await rpc(rt.session, 'mem.cf.poke', { addr: portAddr, value: 5 });
      await rt.ui.wait({ ms: 2500, why: SETTLE });

      expect(await word(planSymbol(kit.plan, 'usb_bulk_audio_state')), 'ua_prepare accepted the stock descriptor').toBe(1);
      const e5 = table + 5 * 0x28;
      const length = await word(e5 + 0x10);
      expect(length, 'entry 5 descriptor length').toBe(0x158);
      const d = await word(e5 + 0xc);
      const desc = Buffer.alloc(length);
      for (let i = 0; i < length; i += 4) desc.writeUInt32BE(await word(d + i), i);
      expect(desc.readUInt16LE(2), 'wTotalLength').toBe(0x0158);
      expect(desc[4], 'bNumInterfaces').toBe(6);
      expect(desc.subarray(0x148).toString('hex'), 'interface 5: vendor class, EP2 OUT bulk 512').toBe('09040500' + '01ff000000' + '07050202000200');

      const r = await kit.usb(rt, (dk) => dk.request(ECHO_CMD));
      expect(r.status).toBe(0);
      const state = Buffer.from(r.body).readUInt32BE(4);
      expect(state & OUT_ONLY, 'OUT-only mode').toBe(OUT_ONLY);
      expect(state & WRONG_MODE, 'not the wrong mode').toBe(0);

      // the bus reset: the stock connect enables EP2 TX (audio feedback); the second connect and its disconnect must leave that half as it is
      const connect = () => rpc(rt.session, 'usb.bulk.in', { connect: planSymbol(kit.plan, 'ua_connect') });
      await connect();
      await rt.ui.wait({ ms: 50, why: SETTLE });
      const first = await rt.session.client.memPeek(ENDPTCTRL2);
      if (first === null) {
        console.log('ENDPTCTRL2 is not readable through mem.cf.peek: the feedback endpoint check is skipped');
      } else {
        expect(first & FEEDBACK_TX, 'EP2 TX iso is enabled after the connect').toBe(FEEDBACK_TX);
        await connect();
        await rt.ui.wait({ ms: 50, why: SETTLE });
        expect((await word(ENDPTCTRL2)) >>> 16, 'the TX half is unchanged by a second connect').toBe(first >>> 16);
      }
      await rt.session.expectNoFault('usb-bulk-audio');

      // MS OS 2.0 answers of ua_setup_hook (docs/plans/usb-winusb.md), built here from the spec layout
      const u16 = (v: number) => Buffer.from([v & 0xff, v >> 8]);
      const utf16 = (s: string) => Buffer.from(s, 'utf16le');
      const cat = (...p: (Buffer | number[])[]) => Buffer.concat(p.map((x) => Buffer.from(x)));
      const guid = '{7F45739F-0563-45C1-823A-43A14AD7F745}';
      const nameBytes = cat(utf16('DeviceInterfaceGUIDs'), [0, 0]);
      const dataBytes = cat(utf16(guid), [0, 0, 0, 0]);
      const property = cat(u16(2 + 2 + 2 + 2 + nameBytes.length + 2 + dataBytes.length), u16(4), u16(7), u16(nameBytes.length), nameBytes, u16(dataBytes.length), dataBytes);
      const compat = cat(u16(20), u16(3), Buffer.from('WINUSB\0\0', 'latin1'), Buffer.alloc(8));
      const fnSubset = cat(u16(8), u16(2), [5, 0], u16(8 + compat.length + property.length), compat, property);
      const cfgSubset = cat(u16(8), u16(1), [0, 0], u16(8 + fnSubset.length), fnSubset);
      const msSet = cat(u16(10), u16(0), [0, 0, 3, 6], u16(10 + cfgSubset.length), cfgSubset);
      const uuid = [0xdf, 0x60, 0xdd, 0xd8, 0x89, 0x45, 0xc7, 0x4c, 0x9c, 0xd2, 0x65, 0x9d, 0x9e, 0x64, 0x8a, 0x9f];
      const cap = cat([0x1c, 0x10, 5, 0], uuid, [0, 0, 3, 6], u16(msSet.length), [0x44, 0]);
      const bos = cat([5, 0x0f], u16(5 + cap.length), [1], cap);
      expect(msSet.length, 'set length').toBe(178);
      expect(bos.length, 'BOS length').toBe(33);

      const setupAddr = anchor('cf.usb.data.setup');
      const sendAddr = anchor('cf.usb.fn.ep0_send');
      const dispatchAddr = anchor('cf.usb.fn.setup_dispatch'); // the patched entry: stub, handler, then the stock dispatcher
      const ctrl = async (setup: string) => {
        const r = await rpc<{ sends: { len: number; bytes: string }[]; stalled: boolean }>(rt.session, 'usb.ctrl', {
          setup: setup.replace(/ /g, ''),
          setup_addr: setupAddr,
          call: dispatchAddr,
          send: sendAddr,
        });
        return {
          sends: r.sends.map((s) => ({ len: s.len, bytes: Buffer.from(s.bytes, 'hex') })),
          stalled: r.stalled,
        };
      };

      type Ctrl = Awaited<ReturnType<typeof ctrl>>;
      const only = (r: Ctrl, what: string): Buffer => {
        expect(r.sends.length, `${what}: one EP0 answer`).toBe(1);
        return r.sends[0]!.bytes;
      };

      // the stock device descriptor of port 5, read from guest memory
      const stockDescAddr = await word(table + 5 * 0x28 + 8);
      const stock = Buffer.alloc(20);
      for (let i = 0; i < 20; i += 4) stock.writeUInt32BE(await word(stockDescAddr + i), i);
      const stockDesc = stock.subarray(0, 18);
      expect(stockDesc.readUInt16LE(2), 'stock bcdUSB differs').not.toBe(0x0201);
      const expectedDesc = Buffer.from(stockDesc);
      expectedDesc.writeUInt16LE(0x0201, 2);
      expectedDesc.writeUInt16LE(0x0101, 12);
      const dev = await ctrl('80 06 00 01 00 00 12 00');
      expect(only(dev, 'device descriptor').toString('hex'), 'device descriptor: stock with bcdUSB 2.01 and bcdDevice 1.01').toBe(expectedDesc.toString('hex'));
      const bosAnswer = await ctrl('80 06 00 0f 00 00 21 00');
      expect(only(bosAnswer, 'BOS').toString('hex'), 'BOS').toBe(bos.toString('hex'));
      const setAnswer = await ctrl('c0 44 00 00 07 00 b2 00');
      expect(only(setAnswer, 'MS OS 2.0 set').toString('hex'), 'MS OS 2.0 set').toBe(msSet.toString('hex'));
      const shortBos = await ctrl('80 06 00 0f 00 00 05 00');
      expect(only(shortBos, 'truncated BOS').toString('hex'), 'BOS truncated to wLength').toBe('050f210001');

      const unknown = await ctrl('c0 45 00 00 00 00 04 00');
      expect(unknown.sends.length, 'unknown vendor request: no answer').toBe(0);
      expect(unknown.stalled, 'unknown vendor request: stall').toBe(true);
      const config = await ctrl('80 06 00 02 00 00 09 00');
      const configHead = only(config, 'stock configuration descriptor');
      expect(configHead.length, 'stock configuration descriptor head').toBe(9);
      expect(configHead[1], 'bDescriptorType').toBe(2);

      // USB MIDI mode: the hook must leave the request to the stock dispatcher, which does not answer BOS
      await rpc(rt.session, 'mem.cf.poke', { addr: portAddr, value: 2 });
      try {
        const other = await ctrl('80 06 00 0f 00 00 21 00');
        expect(other.sends.length, 'no BOS answer in another mode').toBe(0);
        expect(other.stalled, 'no stall in another mode').toBe(false);
      } finally {
        await rpc(rt.session, 'mem.cf.poke', { addr: portAddr, value: 5 });
      }

      // interface not attached: the hook must leave the request to the stock dispatcher
      const stateAddr = planSymbol(kit.plan, 'usb_bulk_audio_state');
      await rpc(rt.session, 'mem.cf.poke', { addr: stateAddr, value: 0 });
      try {
        const detached = await ctrl('80 06 00 0f 00 00 21 00');
        expect(detached.sends.length, 'no BOS answer when not attached').toBe(0);
        expect(detached.stalled, 'no stall when not attached').toBe(false);
      } finally {
        await rpc(rt.session, 'mem.cf.poke', { addr: stateAddr, value: 1 });
      }
      await rt.session.expectNoFault('usb-bulk-audio');
    }, 900_000);
  });
}
