// Which declarations a generated port file (`ports/<dev>-<version>.toml`, written by tools/patchkit/port_resolve.py) changed, and who uses them.
// Every named entry of the file (`[[anchor]]`, `[[hook]]`, ...) comes from a declaration of the same name in `ports/fw/**.toml`, which lists its
// `users`. A change in a few entries selects those users, not every half of the device. A change outside the named entries (`[port]`,
// `[limits]`, `[[region]]`: a new image, other memory limits) or an entry no declaration names can reach anything: it is `wide`, and the caller
// keeps the rule for a device file (every affected half of the device).
import { parse } from 'smol-toml';

const key = (e) => JSON.stringify(e, Object.keys(e ?? {}).sort());

/** `text` -> { named: Map('table:name' -> canonical entry), other: Map(table -> canonical value) }. */
const entriesOf = (text) => {
  const named = new Map();
  const other = new Map();
  for (const [table, v] of Object.entries(text === undefined ? {} : parse(text))) {
    if (Array.isArray(v) && v.every((e) => typeof e?.name === 'string'))
      for (const e of v) named.set(`${table}:${e.name}`, key(e));
    else other.set(table, JSON.stringify(v));
  }
  return { named, other };
};

/** The users of the entries that differ between the generated file `head` and `base` (texts; undefined when absent).
 * `declared`: Map(entry name -> users[]) read from ports/fw. Returns { users: string[], wide: boolean }. */
export const portEntryUsers = ({ head, base, declared }) => {
  const a = entriesOf(base);
  const b = entriesOf(head);
  const tables = new Set([...a.other.keys(), ...b.other.keys()]);
  let wide = [...tables].some((t) => a.other.get(t) !== b.other.get(t));
  const changed = new Set();
  for (const id of new Set([...a.named.keys(), ...b.named.keys()]))
    if (a.named.get(id) !== b.named.get(id)) changed.add(id.slice(id.indexOf(':') + 1));
  const users = new Set();
  for (const name of changed) {
    const who = declared.get(name);
    if (!who?.length) wide = true;
    else who.forEach((u) => users.add(u));
  }
  return { users: [...users], wide };
};

/** Map(entry name -> users[]) of ports/fw declaration texts (every table's entries that have a `name`). */
export const declaredUsers = (texts) => {
  const out = new Map();
  for (const text of texts)
    for (const v of Object.values(parse(text)))
      if (Array.isArray(v))
        for (const e of v) if (typeof e?.name === 'string') out.set(e.name, Array.isArray(e.users) ? e.users : []);
  return out;
};
