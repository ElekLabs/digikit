// Which device is on the link, without the digikit runtime: the stock MidiRpc Ping names the model and SoftwareVersion gives the OS
// (docs/findings/144-stock-device-identity.md). The runtime's INFO then only says whether the runtime is there and which version it is.
import { fmtVersion, type DkInfoFull, type DkSession } from './dkupload-core.ts';

/** The stock MidiRpc type of Ping (docs/MIDI-SYSEX-RPC.md): no payload. */
export const RPC_PING = 0x01;

/** The stock MidiRpc type of SoftwareVersion (docs/MIDI-SYSEX-RPC.md): no payload; the reply holds NUL-terminated strings, one of them the OS version. */
export const RPC_SOFTWARE_VERSION = 0x02;

/**
 * The model name in the reply of Ping. The payload is `u8 blobLen`, the blob of request types the device answers, then NUL-ended strings; the first
 * string is the name. The blob and what follows the name are not read. Undefined when the reply is empty, cut short or has an empty name.
 */
export function parsePing(payload: Uint8Array): string | undefined {
  const first = payload[0];
  if (first === undefined) return undefined;
  const start = 1 + first;
  const end = payload.indexOf(0, start);
  if (start >= payload.length || end < 0 || end === start) return undefined;
  return new TextDecoder('latin1').decode(payload.subarray(start, end));
}

/** The OS version in the reply of SoftwareVersion (`"00"`, `"79"`, `"1.17"`: the string that looks like a version), or undefined. */
export function parseSoftwareVersion(payload: Uint8Array): string | undefined {
  const text = new TextDecoder('latin1').decode(payload);
  return text.split('\0').find((s) => /^\d+\.\d+[0-9A-Za-z.]*$/.test(s));
}

/** What `identifyDevice` needs of the device registry (`Registry` of @digi/devices fits). */
export interface IdentityRegistry {
  readonly devices: readonly { readonly id: string; readonly name: string }[];
}

export interface DeviceIdentity {
  /** The registry id of the device, from Ping. */
  id: string;
  /** The registry name (`[device] name`). */
  name: string;
  /** The OS version, when the device answers SoftwareVersion. */
  os: string | undefined;
  /** The runtime version (`0.5.2`); null when the device has no runtime. */
  runtime: string | null;
  /** The runtime's INFO; null when the device has no runtime. */
  info: DkInfoFull | null;
}

/** The wait for Ping and SoftwareVersion. */
const RPC_MS = 1500;
/** The wait for the runtime's INFO: a device without the runtime does not answer, and this is how long that takes to know. */
const INFO_MS = 1000;

const kindOf = (e: unknown): string | undefined => (e as { kind?: string } | null)?.kind;

/**
 * Identify the device on `s`: Ping names the model (an exact match with a registry name), SoftwareVersion gives the OS, and one short INFO tells
 * whether the runtime is there. No INFO reply in time means no runtime; an INFO error reply (busy, protocol) is thrown, because it proves a runtime
 * answered. The id always comes from Ping.
 */
export async function identifyDevice(
  s: Pick<DkSession, 'rpc' | 'info'>,
  registry: IdentityRegistry,
): Promise<DeviceIdentity> {
  let name: string | undefined;
  try {
    name = parsePing(await s.rpc(RPC_PING, undefined, RPC_MS));
  } catch (e) {
    if (kindOf(e) !== 'timeout') throw e;
    throw new Error('the device does not answer Ping, so it cannot be identified');
  }
  const os = await s.rpc(RPC_SOFTWARE_VERSION, undefined, RPC_MS).then(parseSoftwareVersion, (e: unknown) => {
    if (kindOf(e) === 'cancelled') throw e;
    return undefined;
  });
  const device = registry.devices.find((d) => d.name === name);
  if (!device) {
    const known = registry.devices.map((d) => d.name).join(', ');
    throw new Error(
      `unknown device ${name === undefined ? '(Ping gave no name)' : `"${name}"`}${os ? ` (OS ${os})` : ''}; known devices: ${known}`,
    );
  }
  let info: DkInfoFull | null = null;
  try {
    info = await s.info(INFO_MS);
  } catch (e) {
    if (kindOf(e) !== 'timeout') throw e;
  }
  return { id: device.id, name: device.name, os, runtime: info ? fmtVersion(info.runtime) : null, info };
}
