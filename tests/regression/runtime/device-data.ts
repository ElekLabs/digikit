import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, type Device } from '@digi/test-support';
import { parse } from 'smol-toml';

/** The one reader of devices/<profile>.toml and devices/ports/<device>-<firmware>.toml. Only the keys read by the tests are typed. */
export interface DeviceProfile {
  limits: { fx_slots: number } & Record<string, number>;
  digikit: { id: number; name: string; peek: [number, number][]; dsp_area_bytes: number; sample_path_max: number };
  /** The kit object and its Sound objects (finding 02, 61): `kit_ptr` holds the pointer to the live kit; track t's Sound object is at kit + `sound_off` + `sound_stride` * t, its parameter array 0x14 into it, `sound_words` halfwords long. `lfo_live`: word 0 of track 1 in the array the LFO tick adds its output to (a row is `sound_words` halfwords); `lfo_phase`: the phase accumulator of LFO 1 of voice slot 0 (finding 149), slot s at `lfo_phase_stride` * s, `lfo_slots` slots; `lfo_frame`: Sound index 25 of voice slot 0 in the DSP transmit frame (the final value of each handler run), slot s at `lfo_frame_stride` * s, `lfo_frame_words` indices from 25. */
  fx: {
    kit_ptr: number;
    sound_off: number;
    sound_stride: number;
    sound_words: number;
    lfo_live: number;
    lfo_phase: number;
    lfo_phase_stride: number;
    lfo_slots: number;
    lfo_frame: number;
    lfo_frame_stride: number;
    lfo_frame_words: number;
  };
  /** `[ui]`: `brain_ptr`, the Brain pointer global (the port anchor `cf.view.value.brain`). The view stack and boot window facts are read by `@digi/test-support` `bootWindowFacts`. */
  ui: { brain_ptr: number };
  capabilities: { card: 'path-fs' | 'slots' | 'none' | 'unknown' } & Record<string, string>;
  storage?: { samples?: string };
  card: {
    regions?: { role: string; sector: number }[];
    store: {
      first_sector: number;
      end_sector: number;
      header_b_sector: number;
      selection_sectors?: number;
      settings_sectors?: number;
    };
  };
}

export interface PortRegion {
  name: string;
  cpu: string;
  start: number;
  end: number;
  /** `[lo, hi]` pairs cut out of the region. */
  excludes?: [number, number][];
}

export interface PortFile {
  region?: PortRegion[];
  anchor?: { name: string; addr?: number }[];
}

const PROFILES: Record<Device, string> = { dt2: 'digitakt-ii', dn2: 'digitone-ii' };

const readToml = <T>(...parts: string[]) =>
  parse(fs.readFileSync(path.join(repoRoot(), 'devices', ...parts), 'utf8')) as T;

export const deviceProfile = (device: Device) => readToml<DeviceProfile>(`${PROFILES[device]}.toml`);

export const portFile = (device: Device, firmware: string) => readToml<PortFile>('ports', `${device}-${firmware}.toml`);

/** The module store of a device in absolute card sectors (`end` exclusive), as in tools/patchkit/pk_device.py `store_of`. */
export function cardStore(device: Device) {
  const s = deviceProfile(device).card.store;
  const selection = s.selection_sectors ?? 2;
  const settings = s.settings_sectors ?? 32;
  const setA = s.end_sector - selection - settings;
  return {
    first: s.first_sector,
    end: s.end_sector,
    selA: s.end_sector - selection,
    selB: s.end_sector - selection + 1,
    setA,
    setB: setA + settings / 2,
  };
}

/** The module store in absolute card sectors, with the sectors a store may use (`limit`) and the first sector of the firmware's own project area. */
export function uploadStore(device: Device) {
  const profile = deviceProfile(device);
  const project = profile.card.regions?.find((r) => r.role === 'working-project');
  if (!project) throw new Error(`${device}: no working-project card region`);
  const store = cardStore(device);
  return { ...store, limit: profile.card.store.header_b_sector - store.first, firmwareFirst: project.sector };
}

/** The `cf.expansion` region of the port file: [lo, hi). */
export function expansionRegion(device: Device, firmware: string): [number, number] {
  const region = portFile(device, firmware).region?.find((r) => r.name === 'cf.expansion');
  if (!region) throw new Error(`no cf.expansion region in ${device}-${firmware}`);
  return [region.start, region.end];
}

/** The address of the port anchor `name` (exactly one must exist). */
export function portAnchorAddress(device: Device, firmware: string, name: string): number {
  const anchors = (portFile(device, firmware).anchor ?? []).filter((a) => a.name === name);
  if (anchors.length !== 1) throw new Error(`expected exactly one ${name} anchor, found ${anchors.length}`);
  const address = anchors[0]!.addr;
  if (typeof address !== 'number' || !Number.isInteger(address) || address <= 0)
    throw new Error(`${name}: invalid address`);
  return address;
}

/** The address of the DN2 FX send slot bus, from its port anchor. */
export const slotBusAddress = (device: Device, firmware: string): number =>
  portAnchorAddress(device, firmware, 'dsp.fx.value.send.slot_bus');
