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
import { portAnchorAddress } from './device-data.ts';
import { FIRMWARE } from './devices.ts';

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
/** `DK_UI_LIST_TEXT` of <dk/platform/ui.h> (80): an item is up to 79 characters and its NUL. */
const MAX_TEXT = 80;

/** A COW std::string pointer points at its characters: read up to the NUL. Throws when there is none or a byte is not printable text . */
async function text(read: ReadMemory, ptr: number): Promise<string> {
  const bytes = await read(ptr, MAX_TEXT);
  const end = bytes.indexOf(0);
  if (end < 0) throw new Error(`the string at 0x${ptr.toString(16)} has no end within ${MAX_TEXT} bytes`);
  const out = Buffer.from(bytes.subarray(0, end)).toString('latin1');
  if (!/^[\x20-\x7e]*$/.test(out))
    throw new Error(`the string at 0x${ptr.toString(16)} is not text: ${JSON.stringify(out)}`);
  return out;
}

/** An item as the popup shows it, one space for each run of padding before the colon (`NAME<spaces>: SEND` is `NAME: SEND`): the test compares the text, not the pixels. */
export const plainItem = (item: string): string => item.replace(/ +: /, ': ');

/**
 * The choice popup on top of the firmware's view stack, read from the window object (fields of the port file: item and title vectors of COW strings, selected index), or
 * undefined when nothing is open or the top view is not a MultiChoiceWindow (its vtable is the port's `cf.window.site.multichoice_vptr_lit`). The text of the window is what the
 * firmware draws. A top view of that class that cannot be read (a bad vector, text that is not text, a failed read) throws: the caller then reports it instead of waiting.
 */
export async function readChoiceWindow(
  session: Pick<Session, 'client' | 'options'>,
): Promise<ChoiceWindow | undefined> {
  const facts = viewStackFacts(session.options.device as Device);
  if (!facts) throw new Error(`${session.options.device} has no [ui] facts`);
  const read = emulatorRead(session);
  const device = session.options.device as Device;
  const vptr = portAnchorAddress(device, FIRMWARE[device], 'cf.window.site.multichoice_vptr_lit');
  const word = async (addr: number) => new DataView((await read(addr, 4)).slice().buffer).getUint32(0);
  const [top] = await readViewStack(read, facts);
  if (!top || top.vptr !== vptr) return undefined;
  const vector = async (what: string, begin: number, end: number) => {
    const b = await word(top.view + begin);
    const e = await word(top.view + end);
    const n = (e - b) / 4;
    if (b === 0 || e < b || !Number.isInteger(n) || n < 1 || n > MAX_ITEMS)
      throw new Error(
        `the ${what} vector of the choice window at 0x${top.view.toString(16)} is bad: begin 0x${b.toString(16)}, end 0x${e.toString(16)}`,
      );
    const lines: string[] = [];
    for (let i = 0; i < n; i++) lines.push(await text(read, await word(b + 4 * i)));
    return lines;
  };
  const items = await vector('items', windowField('items_begin'), windowField('items_end'));
  const title = await vector('title', windowField('title_begin'), windowField('title_end'));
  return { title: title.join('\n'), items, selected: await word(top.view + windowField('selected')) };
}
