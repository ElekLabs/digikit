import { parse } from 'smol-toml';
import sdk from '../../../../sdk/sdk.toml?raw';

const { runtime } = parse(sdk) as { runtime: { version: string } };
export const previewRuntimeVersion = runtime.version;

export function packedVersion(version: string): number {
  return version.split('.').reduce((word, part) => (word << 8) | Number(part), 0);
}
