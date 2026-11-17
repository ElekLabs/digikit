// The desktop shell supplies the host's emulator RPC to the shared USB SysEx transport.
import { EmuUsbTransport as RpcUsbTransport } from '@digi/dk-session';
import type { EmuRpc } from './host';

export { EMU_RPC_CHUNK } from '@digi/dk-session';

export class EmuUsbTransport extends RpcUsbTransport {
  constructor(rpc: EmuRpc) {
    super((method, params) => rpc.call(method, params));
  }
}

/** The controller's run mode: a paused emulator would never answer (nothing steps it). */
export async function emulatorMode(rpc: EmuRpc): Promise<string | undefined> {
  return (await rpc.call<{ mode?: string }>('status')).mode;
}
