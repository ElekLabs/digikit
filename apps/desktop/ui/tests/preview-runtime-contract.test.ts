// The preview's simulated runtime keeps the capability contract of the real one: INFO advertises a capability, and every command of it is answered in
// the format of sdk.toml and decodes with the parsers Workbench uses; a command whose capability is not advertised is not registered, so it gets no reply.
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DkSession, hasCap, readSettingsRecord, type DkTransport } from '@digi/dk-session';
import { CAP, CMD, SERVICE_CMD } from '@digi/dk-session/gen_proto';

const fixture = { dev: 'dt2', state: 'runtime', os: '1.17', flashMs: 0, goneMs: 0, sent: 0 };
type Cap = keyof typeof CAP;

// Every capability bit of sdk.toml `[protocol.caps]` and the commands it gates (`[protocol.commands]`, `[protocol.service]`). A capability that gates
// no command of its own has an empty list; a new bit in CAP fails the first test until it is listed here.
const COMMANDS: Record<Cap, readonly number[]> = {
  LOG: [SERVICE_CMD.LOG],
  PEEK: [SERVICE_CMD.PEEK],
  LOGCFG: [SERVICE_CMD.LOGCFG],
  SELECT: [CMD.SELECT],
  RESTART: [CMD.RESTART],
  MODULES: [CMD.MODULES],
  CAPACITY: [CMD.CAPACITY],
  READ: [CMD.READ],
  VERIFY: [CMD.VERIFY],
  SERVICE_CMDS: [],
  FOLDERS: [CMD.MKDIR],
  SETTINGS: [CMD.SETTINGS],
  FILEHASH: [],
  RENAME: [CMD.RENAME],
  READBACK: [],
  LOADS: [CMD.LOADS],
  SVC_ABI: [],
  RELOAD: [CMD.RELOAD, CMD.RELOAD_STATE],
  FW_ID: [],
  LIVE: [CMD.LOAD, CMD.UNLOAD],
  MODULE_UI: [CMD.MODULE_UI],
  MODULE_SECTIONS: [],
  MODULE_QUERIES: [],
};

// What Workbench does with each reply: the real parser of this command, or none yet (the command is then only checked for a reply).
const PARSERS: Partial<Record<number, (dk: DkSession) => Promise<unknown>>> = {
  [SERVICE_CMD.LOG]: (dk) => dk.logPage(0),
  [SERVICE_CMD.PEEK]: (dk) => dk.peek(0x1000, 4),
  [SERVICE_CMD.LOGCFG]: async (dk) => [await dk.logConfig(), await dk.cardLog()],
  [CMD.SETTINGS]: (dk) => readSettingsRecord(dk),
  [CMD.MODULES]: (dk) => dk.modules(),
  [CMD.CAPACITY]: (dk) => dk.capacity(),
  [CMD.LOADS]: (dk) => dk.loadStates(),
};

const hexBytes = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16));

async function runtime(modules: string[]) {
  vi.stubGlobal('location', { search: '' }); // the preview's sim reads the page query
  const { createFirmwareMidi } = await import('../preview/firmware-midi');
  const midi = createFirmwareMidi(new URLSearchParams(), fixture);
  midi.readCard(async () => modules);
  const pending: Uint8Array[] = [];
  const drain = (hex: string) => {
    let start = 0;
    const bytes = hexBytes(hex);
    bytes.forEach((b, i) => {
      if (b !== 0xf7) return;
      pending.push(bytes.subarray(start, i + 1));
      start = i + 1;
    });
  };
  const transport: DkTransport = {
    send: (msg) => midi.usbIn('dt2', msg),
    async recv(timeoutMs) {
      const end = Date.now() + timeoutMs;
      for (;;) {
        drain(midi.usbOut());
        const next = pending.shift();
        if (next) return next;
        if (Date.now() >= end) return null;
        await new Promise((r) => setTimeout(r, 2));
      }
    },
  };
  const dk = new DkSession(transport, { timeoutMs: 60, retries: 0 });
  // a command answered or not: the request's reply status, or null when nothing came back
  const sent = async (cmd: number, body: number[] = [0, 0, 0, 0, 0, 4]) =>
    dk.request(cmd, Uint8Array.from(body), 60).then(
      (r) => r.status,
      () => null,
    );
  return { dk, sent };
}

afterEach(() => vi.unstubAllGlobals());

describe.each([
  ['without digikit/debug', ['digikit/pluck']],
  ['with digikit/debug', ['digikit/debug']],
])('the preview runtime %s', (_name, modules) => {
  it('answers every command of an advertised capability in its format and no other command', async () => {
    const { dk, sent } = await runtime(modules);
    const info = await dk.info();
    const names = Object.keys(CAP) as Cap[];
    expect(Object.keys(COMMANDS).sort()).toEqual([...names].sort());
    expect(hasCap(info, 'LOG')).toBe(modules.includes('digikit/debug'));
    for (const name of names) {
      for (const cmd of COMMANDS[name]) {
        const label = `${name} command 0x${cmd.toString(16)}`;
        if (hasCap(info, name)) {
          expect(await sent(cmd), `${label} is advertised and gets a reply`).not.toBeNull();
          const parse = PARSERS[cmd];
          if (parse) await expect(parse(dk), `${label} decodes`).resolves.toBeDefined();
        } else expect(await sent(cmd), `${label} is not advertised and gets no reply`).toBeNull();
      }
    }
  });
});
