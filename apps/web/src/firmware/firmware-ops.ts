// The operations of a flash (firmware-flow.ts `FlowOps`) over the app's MIDI port: the selected target's transport sends the messages, the
// port list tells when the device has gone and come back, and the session asks it afterwards. Used by the page in both apps.
import { transferEnv } from '../transfer/env';
import { portTransport, releasePort, session, target } from '../transfer/target';
import { firmwareEnv } from './env';
import { askSession } from './firmware-ask';
import type { Answer } from './firmware-core';
import type { FlowOps } from './firmware-flow';
import { pause } from './pause';

/** What the device on the selected port says (see firmware-ask.ts). */
export async function askDevice(): Promise<Answer & { port: string; model?: string }> {
  const t = target();
  if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
  return { port: t.port, ...(await askSession(await session())) };
}

/** The operations for the selected MIDI port. Call it after the port is open (the transport is taken at the first `send`). */
export function portOps(): FlowOps {
  const t = target();
  if (t.kind !== 'midi') throw new Error('No MIDI port is selected.');
  const port = t.port;
  let link: ReturnType<typeof portTransport> | undefined;
  return {
    sender: (total) => firmwareEnv().sender(total),
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
    portPresent: async () => (await transferEnv().targets.ports()).includes(port),
    async ask() {
      const { os, runtime, imageId } = await askDevice();
      return { os, runtime, imageId };
    },
  };
}
