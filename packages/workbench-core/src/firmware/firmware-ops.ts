// The operations of a flash (firmware-flow.ts `FlowOps`) over the app's MIDI port: the selected target's transport sends the messages, the
// port list tells when the device has gone and come back, and the session asks it afterwards. Used by the page in both apps.
import type { TransferApp } from '../transfer/app';
import type { FirmwareEnv } from './env';
import { askSession } from './firmware-ask';
import type { Answer } from './firmware-core';
import type { FlowOps, RpcOps } from './firmware-flow';
import { pause } from './pause';

export function createFirmwareOps(env: FirmwareEnv, transfer: TransferApp) {
  const { portTransport, releasePort, selectTarget, session, target } = transfer.target;
  /** What the device on the selected port says (see firmware-ask.ts). */
  async function askDevice(): Promise<Answer & { port: string; model?: string }> {
    const t = target();
    if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
    return { port: t.port, ...(await askSession(await session())) };
  }

  /** The operations for the selected MIDI port. Call it after the port is open (the transport is taken at the first `send`). */
  function portOps(): FlowOps {
    const t = target();
    if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
    let port = t.port;
    let link: ReturnType<typeof portTransport> | undefined;
    return {
      sender: (total) => env.sender(total),
      async send(m) {
        link ??= portTransport();
        await (await link).send(m);
      },
      now: () => performance.now(),
      sleep: pause,
      release: async () => {
        link = undefined;
        await releasePort();
      },
      get port() {
        return port;
      },
      ports: () => transfer.env.targets.ports(),
      async moved(name) {
        // the next requests (this run, the Transfer pane, the Log) use the port under its new name
        port = name;
        link = undefined;
        await selectTarget({ kind: 'midi', port: name });
      },
      log: (text) => env.log({ level: 'info', text }),
      async ask() {
        const { os, runtime, imageId } = await askDevice();
        return { os, runtime, imageId };
      },
    };
  }

  /** The operations of the RPC route for the selected MIDI port: the machine, and the port's transport for one message out and the next one in. */
  function rpcPortOps(): RpcOps {
    const base = portOps();
    let link: ReturnType<typeof portTransport> | undefined;
    const transport = () => (link ??= portTransport());
    return {
      machine: (image, chunk) => env.rpc(image, chunk),
      send: async (m) => (await transport()).send(m),
      recv: async (ms, signal) => (await transport()).recv(ms, signal),
      now: base.now,
      sleep: base.sleep,
      release: async () => {
        link = undefined;
        await base.release();
      },
      get port() {
        return base.port;
      },
      ports: base.ports,
      moved: async (name) => {
        link = undefined;
        await base.moved?.(name);
      },
      log: base.log,
      ask: base.ask,
    };
  }

  return { askDevice, portOps, rpcPortOps };
}
export type FirmwareOps = ReturnType<typeof createFirmwareOps>;
