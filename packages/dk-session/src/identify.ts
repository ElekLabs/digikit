// Which device is on the link, without the digikit runtime: the stock MidiRpc Ping names the model and SoftwareVersion gives the OS
// (docs/findings/144-stock-device-identity.md). The runtime's INFO then only says whether the runtime is there and which version it is.
import { fmtVersion, INFO_PROBE_TIMEOUT_MS, type DkInfoFull, type DkSession } from './dkupload-core.ts';

/** The stock MidiRpc type of Ping (docs/MIDI-SYSEX-RPC.md): no payload. */
export const RPC_PING = 0x01;

/** The stock MidiRpc type of SoftwareVersion (docs/MIDI-SYSEX-RPC.md): no payload; the reply holds NUL-terminated strings, one of them the OS version. */
export const RPC_SOFTWARE_VERSION = 0x02;

/**
 * The model name in the reply of Ping. Two layouts, told apart by the framing and not by their content:
 * - plain: `u8 blobLen`, the blob of request types, NUL-ended strings (the first is the name);
 * - length-prefixed: `u8 rest`, then the plain layout, where `rest` is the number of bytes after the `blobLen` byte (the Syntakt 1.42 reply,
 *   `1e 16 <22 types> "Syntakt" 00`, finding 144). A payload whose first byte is two less than its length is this one.
 * Undefined when the payload is empty, cut short, or the first string is not printable ASCII. The vectors are `tests/fixtures/ping-vectors.json`, shared with `parse_ping` of
 * `packages/patchkit/src/flash_rpc.rs`.
 */
export function parsePing(payload: Uint8Array): string | undefined {
  const body = payload.length >= 2 && payload[0] === payload.length - 2 ? payload.subarray(1) : payload;
  const first = body[0];
  if (first === undefined) return undefined;
  const start = 1 + first;
  const end = body.indexOf(0, start);
  if (start >= body.length || end < 0 || end === start) return undefined;
  const name = body.subarray(start, end);
  // a name is printable text: an unknown or damaged reply gives no name, not junk
  return name.every((b) => b >= 0x20 && b < 0x7f) ? new TextDecoder('latin1').decode(name) : undefined;
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
const kindOf = (e: unknown): string | undefined => (e as { kind?: string } | null)?.kind;

async function probeRuntime(s: Pick<DkSession, 'info'>): Promise<DkInfoFull | null> {
  try {
    return await s.info({ timeoutMs: INFO_PROBE_TIMEOUT_MS, retries: 1 });
  } catch (e) {
    if (kindOf(e) !== 'timeout') throw e;
    return null;
  }
}

function runtimeDevice(info: DkInfoFull | null, registry: IdentityRegistry) {
  return registry.devices.find((d) => info?.port === d.id || info?.port.startsWith(d.id + '-'));
}

/**
 * Identify the device on `s`: Ping names the model (an exact match with a registry name), SoftwareVersion gives the OS, and one INFO tells whether
 * the runtime is there. INFO retries E_BUSY like any INFO and has one retry after a lost reply; only a lost reply means no runtime. Any other INFO
 * error reply is thrown, because it proves a runtime answered. The id comes from Ping; when Ping is silent (the stock firmware drops requests during
 * its intro) but the runtime answers INFO, the id comes from INFO's port, matched against the registry.
 */
export async function identifyDevice(
  s: Pick<DkSession, 'rpc' | 'info'>,
  registry: IdentityRegistry,
  options: { preferRuntimeInfo?: boolean } = {},
): Promise<DeviceIdentity> {
  // A runtime panel only needs the model and runtime. INFO alone supplies both.
  const probed = options.preferRuntimeInfo ? await probeRuntime(s) : undefined;
  const known = probed && runtimeDevice(probed, registry);
  if (known && probed)
    return { id: known.id, name: known.name, info: probed, runtime: fmtVersion(probed.runtime), os: undefined };
  let name: string | undefined;
  let pingSilent = false;
  try {
    name = parsePing(await s.rpc(RPC_PING, undefined, RPC_MS));
  } catch (e) {
    if (kindOf(e) !== 'timeout') throw e;
    pingSilent = true;
  }
  const info = probed === undefined ? await probeRuntime(s) : probed;
  if (pingSilent && !info) throw new Error('the device does not answer Ping, so it cannot be identified');
  const os = pingSilent
    ? undefined
    : await s.rpc(RPC_SOFTWARE_VERSION, undefined, RPC_MS).then(parseSoftwareVersion, (e: unknown) => {
        if (kindOf(e) === 'cancelled') throw e;
        return undefined;
      });
  const device = pingSilent ? runtimeDevice(info, registry) : registry.devices.find((d) => d.name === name);
  if (!device) {
    const known = registry.devices.map((d) => d.name).join(', ');
    const what = pingSilent
      ? `(Ping is silent, INFO port "${info?.port}")`
      : name === undefined
        ? '(Ping gave no name)'
        : `"${name}"`;
    throw new Error(`unknown device ${what}${os ? ` (OS ${os})` : ''}; known devices: ${known}`);
  }
  return { id: device.id, name: device.name, os, runtime: info ? fmtVersion(info.runtime) : null, info };
}
