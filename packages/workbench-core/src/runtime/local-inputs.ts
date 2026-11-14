/** Dev-server-only private inputs (`/__digi-local/`); production has no such route. */
export interface LocalInputs {
  image: Uint8Array;
  state: Uint8Array;
  snapshot?: Uint8Array;
}
export interface LocalManifest {
  dspImage: boolean;
  dspState: boolean;
  cfSnapshot: boolean;
  firmware?: boolean;
}

export async function localManifest(): Promise<LocalManifest | undefined> {
  try {
    // A dev-server route at the server root, not under BASE_URL.
    const response = await fetch('/__digi-local/manifest', { cache: 'no-store' });
    if (!response.ok) return undefined;
    const value = await response.json();
    return value && typeof value === 'object' ? (value as LocalManifest) : undefined;
  } catch {
    return undefined;
  }
}

const fetchBytes = async (name: string) => {
  try {
    const response = await fetch(`/__digi-local/${name}`, { cache: 'no-store' });
    if (
      !response.ok ||
      !(response.headers.get('content-type') ?? '').toLowerCase().startsWith('application/octet-stream')
    )
      return undefined;
    return new Uint8Array(await response.arrayBuffer());
  } catch {
    return undefined;
  }
};

export async function fetchLocalInputs(): Promise<LocalInputs | undefined> {
  const manifest = await localManifest();
  if (!manifest?.dspImage || !manifest.dspState) return undefined;
  const [image, state, snapshot] = await Promise.all([
    fetchBytes('dsp-image'),
    fetchBytes('dsp-state'),
    manifest.cfSnapshot ? fetchBytes('cf-snapshot') : Promise.resolve(undefined),
  ]);
  return image && state ? { image, state, snapshot } : undefined;
}

/** The firmware .syx named by DIGI_EMU_SYX (bench only; normal use picks the file in the page). */
export const fetchLocalFirmware = () => fetchBytes('firmware');

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', bytes.slice());
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('');
}
