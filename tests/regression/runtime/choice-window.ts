import fs from 'node:fs';
import path from 'node:path';
import { parse as parseToml } from 'smol-toml';
import {
  emulatorRead,
  readViewStack,
  repoRoot,
  viewStackFacts,
  type Device,
  type ReadMemory,
  type Session,
} from '@digi/test-support';

/** What a stock MultiChoiceWindow (the firmware's choice popup, `dk_ui_choice`) shows: the title line, the items and the selected index. */
export interface ChoiceWindow {
  title: string;
  items: string[];
  selected: number;
}

/** The offset of `field` in `cf.window.struct.MultiChoiceWindow` of ports/fw/window.toml. */
function windowField(field: string): number {
  const rules = parseToml(fs.readFileSync(path.join(repoRoot(), 'ports/fw/window.toml'), 'utf8')) as {
    struct: { name: string; fields: { name: string; off: number }[] }[];
  };
  const f = rules.struct
    .find((x) => x.name === 'cf.window.struct.MultiChoiceWindow')
    ?.fields.find((x) => x.name === field);
  if (!f) throw new Error(`ports/fw/window.toml: no MultiChoiceWindow.${field}`);
  return f.off;
}

const MAX_ITEMS = 16;
const MAX_TEXT = 40;

/** A COW std::string pointer points at its characters: read up to the NUL, or undefined when the bytes are not printable text. */
async function text(read: ReadMemory, ptr: number): Promise<string | undefined> {
  const bytes = await read(ptr, MAX_TEXT);
  const end = bytes.indexOf(0);
  if (end < 0) return undefined;
  const out = Buffer.from(bytes.subarray(0, end)).toString('latin1');
  return /^[\x20-\x7e]*$/.test(out) ? out : undefined;
}

/**
 * The choice popup on top of the firmware's view stack, read from the window object (fields of the port file: item and title vectors of COW strings, selected index), or
 * undefined when the top view holds no readable choice (nothing is open, or the top view is another class). The text of the window is what the firmware draws.
 */
export async function readChoiceWindow(
  session: Pick<Session, 'client' | 'options'>,
): Promise<ChoiceWindow | undefined> {
  const facts = viewStackFacts(session.options.device as Device);
  if (!facts) throw new Error(`${session.options.device} has no [ui] facts`);
  const read = emulatorRead(session);
  const word = async (addr: number) => new DataView((await read(addr, 4)).slice().buffer).getUint32(0);
  try {
    const [top] = await readViewStack(read, facts);
    if (!top) return undefined;
    const vector = async (begin: number, end: number) => {
      const b = await word(top.view + begin);
      const e = await word(top.view + end);
      const n = (e - b) / 4;
      if (b === 0 || e < b || !Number.isInteger(n) || n < 1 || n > MAX_ITEMS) return undefined;
      const lines: string[] = [];
      for (let i = 0; i < n; i++) {
        const line = await text(read, await word(b + 4 * i));
        if (line === undefined) return undefined;
        lines.push(line);
      }
      return lines;
    };
    const items = await vector(windowField('items_begin'), windowField('items_end'));
    const title = await vector(windowField('title_begin'), windowField('title_end'));
    if (!items || !title) return undefined;
    return { title: title.join('\n'), items, selected: await word(top.view + windowField('selected')) };
  } catch {
    return undefined;
  }
}
