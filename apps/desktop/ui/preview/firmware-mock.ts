// The preview's Firmware page prepares images with the real patchkit WASM and module bundle.
import { previewHome } from './preview-home';
import { PatchKit } from '@digi/workbench-core/patch/patchkit-core.ts';
import { deviceProfiles } from '@digi/workbench-core/data/device-profiles.ts';
import { createFirmwareMidi, portOf } from './firmware-midi';
import bundleUrl from '../../../../dist/workbench-preview/bundle.json?url';
import wasmUrl from '../../../web/public/patchkit.wasm?url';

const q = new URLSearchParams(location.search);
const dev = q.get('fwdev') === 'dt2' ? 'dt2' : 'dn2';
export const firmwarePort = portOf(deviceProfiles[dev]);
export const fw = {
  dev,
  state: q.get('fwstate') ?? 'stock',
  os: q.get('fwos') ?? deviceProfiles[dev].firmware.at(-1)!.version,
  flashMs: Number(q.get('fwflash') ?? 3000),
  goneMs: Number(q.get('fwgone') ?? 4000),
  sent: 0,
  lastImageId: null as string | null,
  saved: [] as { name: string; size: number }[],
};
export const midi = createFirmwareMidi(q, fw);

let kit: Promise<PatchKit> | undefined;
async function patchKit(): Promise<PatchKit> {
  kit ??= (async () => {
    const [wasm, bundle] = await Promise.all([fetch(wasmUrl), fetch(bundleUrl)]);
    return PatchKit.create(await wasm.arrayBuffer(), await bundle.text());
  })();
  return kit;
}

export const patchkitWasm = async (): Promise<ArrayBuffer> => (await fetch(wasmUrl)).arrayBuffer();

/** `firmware_prepare`: the real checks, packed as the Tauri command packs them. */
export async function prepare(stock: Uint8Array, restore: boolean, set = ''): Promise<ArrayBuffer> {
  const k = await patchKit();
  const p = k.prepare(stock, restore, set);
  fw.lastImageId = p.imageId;
  midi.prepare(p);
  if (q.get('fwforce') === '1') {
    // every check green: for the screenshots of the steps after the checks when the module bundle and patchkit.wasm of this tree do not match
    for (const c of p.checks) c.ok = true;
    p.ok = true;
  }
  if (q.get('fwfail') === '1') {
    // a failed check cannot be made from a real stock file: the screenshot of one changes the answer
    const c = p.checks.find((x) => x.name === 'DSP scan')!;
    c.ok = false;
    c.detail =
      '2 store(s) the SHARC+ ISA does not allow in section 7 (finding 49 section 10): block 103 pc 0xb8d6a9: DM store of I0 (DAG1); block 103 pc 0xb8d6ab: DM store of I1 (DAG1)';
    p.ok = false;
  }
  const json = new TextEncoder().encode(
    JSON.stringify({
      ok: p.ok,
      restore: p.restore,
      device: p.device,
      os: p.os,
      sha_stock: p.shaStock,
      sha_image: p.shaImage,
      image_id: p.imageId,
      runtime: p.runtime,
      pace_ms: p.paceMs,
      ends: p.ends,
      checks: p.checks,
    }),
  );
  const out = new Uint8Array(4 + json.length + p.image.length);
  new DataView(out.buffer).setUint32(0, json.length);
  out.set(json, 4);
  out.set(p.image, 4 + json.length);
  return out.buffer;
}

export function save(name: string, bytes: Uint8Array): string {
  fw.saved.push({ name, size: bytes.length });
  return `${previewHome}/Downloads/${name}`;
}

(window as unknown as { __fw: typeof fw }).__fw = fw;
