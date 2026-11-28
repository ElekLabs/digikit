// The preview's named MIDI ports. DkSession probes these devices and the firmware sender uses the same wire.
import { decode8in7, encode8in7, RPC_PING, RPC_SOFTWARE_VERSION } from '@digi/dk-session';
import { CMD, PROTO_VERSION } from '@digi/dk-session/gen_proto';
import { deviceProfiles, runtimeDeviceIds, type DeviceProfile } from '@digi/workbench-core/data/device-profiles.ts';
import type { PreparedFlash } from '@digi/workbench-core/patch/patchkit-core.ts';
import { FakeUpgrade } from '@digi/workbench-core/testing/fakeflash.ts';
import { packedVersion, previewRuntimeVersion } from './runtime-version';
import { sim } from './sim';

const virtualPort = 'IAC Driver Bus 1';
export const portOf = (profile: DeviceProfile) => `Elektron ${profile.name}`;

const concat = (...parts: ArrayLike<number>[]) => Uint8Array.from(parts.flatMap((p) => Array.from(p)));
const cs = (s: string) => concat(new TextEncoder().encode(s), [0]);
const be32 = (n: number) => Uint8Array.of(n >>> 24, n >>> 16, n >>> 8, n);
type FirmwareFixture = {
  dev: string;
  state: string;
  os: string;
  flashMs: number;
  goneMs: number;
  sent: number;
};
type Runtime = { version: string; imageId: string | null };
type Image = Pick<PreparedFlash, 'os' | 'restore' | 'runtime' | 'imageId'>;
type Device = {
  profile: DeviceProfile;
  state: string;
  os: string;
  runtime: Runtime | null;
  offlineUntil: number;
  upgrade: FakeUpgrade;
};
type Link = { name: string; queue: Uint8Array[]; wake?: () => void };

export function createFirmwareMidi(query: URLSearchParams, fixture: FirmwareFixture) {
  const page = query.get('page');
  const firmwareScene = page === 'firmware';
  const configuredFixture = firmwareScene || query.has('fwstate') || query.has('fwos');
  const unidentified = query.has('unidentified');
  const profiles = firmwareScene
    ? [deviceProfiles[fixture.dev]]
    : page === 'run' && !query.has('fwdev')
      ? [deviceProfiles[fixture.dev]]
      : runtimeDeviceIds.map((id) => deviceProfiles[id]);
  const devices = new Map<string, Device>(
    profiles.map((profile) => {
      const configured = configuredFixture && profile.short === fixture.dev;
      const state = configured ? fixture.state : query.has('stock') || unidentified ? 'stock' : 'runtime';
      const runtime: Runtime | null =
        state === 'runtime'
          ? { version: configured ? '0.5.2' : previewRuntimeVersion, imageId: configured ? null : '0123456789abcdef' }
          : null;
      return [
        portOf(profile),
        {
          profile,
          state,
          os: configured ? fixture.os : (profile.firmware.at(-1)?.version ?? ''),
          runtime,
          offlineUntil: 0,
          upgrade: new FakeUpgrade({
            writeMs: Number(query.get('fwwrite') ?? 0),
            checkMs: Number(query.get('fwcheck') ?? 1200),
            endMs: Number(query.get('fwend') ?? 40),
            errorAt: query.has('fwerror') ? Number(query.get('fwerror')) : null,
          }),
        },
      ];
    }),
  );
  const images = new Map<string, Image>();
  let link: Link | undefined;

  const online = (device: Device) => device.state !== 'none' && Date.now() >= device.offlineUntil;
  const ports = () => [
    ...[...devices].filter(([, device]) => online(device)).map(([name]) => name),
    ...(page === 'run' ? [] : [virtualPort]),
  ];

  function push(to: Link, message: Uint8Array) {
    if (link !== to) return;
    to.queue.push(message);
    to.wake?.();
  }

  function restart(device: Device) {
    const image = images.get(device.profile.short);
    if (!image) return;
    setTimeout(() => {
      device.offlineUntil = Date.now() + fixture.goneMs;
      device.os = image.os;
      device.runtime = image.restore
        ? null
        : { version: image.runtime ?? previewRuntimeVersion, imageId: image.imageId };
    }, fixture.flashMs);
  }

  function stock(to: Link, device: Device, msg: Uint8Array) {
    // The OS UPGRADE menu accepts a header, data messages and a footer.
    if (msg[6] === 0x7e || msg[6] === 0x7f) {
      if (msg[6] === 0x7f && msg[7] === 1) fixture.sent = 0;
      else if (msg[6] === 0x7e) fixture.sent++;
      else if (msg[7] === 2) restart(device);
      return;
    }
    const body = decode8in7(msg.subarray(6, msg.length - 1));
    const seq = (body[0] << 8) | body[1];
    const type = body[4];
    switch (type) {
      case RPC_PING:
        push(to, device.upgrade.reply(seq, type, concat([0], cs(device.profile.name))));
        return;
      case RPC_SOFTWARE_VERSION:
        push(to, device.upgrade.reply(seq, type, concat(cs('00'), cs('79'), cs(device.os))));
        return;
    }
    const answer = device.upgrade.handle(msg);
    if (!answer) return;
    setTimeout(() => {
      push(to, answer.message);
      if (answer.commit === 1) restart(device);
    }, answer.delayMs);
  }

  function info(to: Link, device: Device, seq: number) {
    if (!device.runtime) return;
    const { version, imageId } = device.runtime;
    const runtime = packedVersion(version);
    const id = (imageId?.match(/../g) ?? []).map((byte) => parseInt(byte, 16));
    const body = concat(
      [0, 0, PROTO_VERSION, (runtime >>> 8) & 255, runtime & 255],
      cs(device.profile.short),
      be32(1 << 20),
      be32(32768),
      [0],
      be32(1),
      be32(1),
      be32(runtime),
      be32(0),
      id,
    );
    push(to, concat([0xf0, 0x7d, 0x64, 0x6b, 1, CMD.INFO | 0x40, seq], encode8in7(body), [0xf7]));
  }

  /** One reply of the runtime: status byte, then the body. */
  function reply(to: Link, cmd: number, seq: number, status: number, body: ArrayLike<number> = []) {
    push(to, concat([0xf0, 0x7d, 0x64, 0x6b, 1, cmd | 0x40, seq], encode8in7(concat([status], body)), [0xf7]));
  }

  // The settings record of a runtime that has stored nothing: the `DKST` header (version 1, seq 0, used 18, crc 0), no entries.
  const settingsRecord = concat(new TextEncoder().encode('DKST'), [0, 1], be32(0), be32(18), be32(0));

  /** SETTINGS (a read in pieces: offset u32, length u16; the reply echoes both and gives the total) and FAULT (no fault held). */
  function settings(to: Link, device: Device, msg: Uint8Array) {
    if (!device.runtime) return;
    const seq = msg[6]!;
    if (msg[5] === CMD.FAULT) return reply(to, CMD.FAULT, seq, 0);
    const request = decode8in7(msg.subarray(7, msg.length - 1));
    const offset = (request[0]! << 24) | (request[1]! << 16) | (request[2]! << 8) | request[3]!;
    const length = (request[4]! << 8) | request[5]!;
    const piece = settingsRecord.subarray(offset, offset + length);
    reply(to, CMD.SETTINGS, seq, 0, concat(request.subarray(0, 6), be32(settingsRecord.length), piece));
  }

  function close() {
    const previous = link;
    link = undefined;
    previous?.wake?.();
  }

  return {
    ports,
    prepare({ device, os, restore, runtime, imageId }: Image & { device: string }) {
      images.set(device, { os, restore, runtime, imageId });
    },
    open(name: string) {
      if (!ports().includes(name)) throw new Error(`The MIDI port ${name} is unavailable.`);
      close();
      link = { name, queue: [] };
    },
    send(msg: Uint8Array) {
      if (!link) throw new Error('The MIDI port is closed.');
      const device = devices.get(link.name);
      if (!device || !online(device) || device.state === 'noreply' || sim.dev === 'noreply') return;
      if (msg[1] === 0x00 && msg[2] === 0x20) stock(link, device, msg);
      else if (msg[1] === 0x7d && msg[5] === CMD.INFO) info(link, device, msg[6]!);
      else if (msg[1] === 0x7d && (msg[5] === CMD.SETTINGS || msg[5] === CMD.FAULT)) settings(link, device, msg);
    },
    async recv(timeoutMs: number): Promise<ArrayBuffer> {
      const from = link;
      if (!from) return new ArrayBuffer(0);
      if (!from.queue.length) {
        let timer: ReturnType<typeof setTimeout> | undefined;
        await new Promise<void>((resolve) => {
          from.wake = resolve;
          timer = setTimeout(resolve, timeoutMs);
        });
        clearTimeout(timer);
        from.wake = undefined;
      }
      const next = link === from ? from.queue.shift() : undefined;
      return next ? next.slice().buffer : new ArrayBuffer(0);
    },
    cancel() {
      link?.wake?.();
    },
    close,
  };
}
