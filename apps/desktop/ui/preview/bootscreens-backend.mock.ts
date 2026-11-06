import { noteConnection } from '@web/transfer/connection-state';
import { setDeviceInfo } from '@web/transfer/transfer-state';
import { delay, deviceRead, sim } from './sim';

let size: number | undefined = sim.dev === 'empty' ? undefined : 2166;
const s = {
  async put(_path: string, bytes: Uint8Array, progress?: (sent: number, total: number) => void) {
    for (let sent = 0; sent < bytes.length; sent += 512) {
      await delay(80);
      progress?.(Math.min(sent + 512, bytes.length), bytes.length);
    }
    size = bytes.length;
  },
  async verifyFile() {
    await delay(200);
    return true;
  },
  async delete() {
    size = undefined;
  },
};
export async function openBootscreens() {
  await deviceRead();
  noteConnection({ type: 'answered', device: { id: 'dn2', name: 'Digitone II', runtime: '0.3.0' } });
  return { s, name: 'Digitone II', runtime: '0.3.0' };
}
export async function readActiveBootscreen() {
  const { name, runtime } = await openBootscreens();
  setDeviceInfo({ name, runtime });
  return size === undefined ? undefined : { size };
}
