// Key-ABI check: the numbers that services/shortcuts/src/key_abi.rs copies (magic, version, KEYS_FULL, KEYS_DUPLICATE) must be those of
// the header of the image's module, runtime/platform/input/src/cf/key_subscribers.h (the one source). A service build stages only its
// crates, so the Rust side cannot read the header; this check fails on any difference. `node tools/nx/key-abi.mjs`.
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const HEADER = 'runtime/platform/input/src/cf/key_subscribers.h';
const RUST = 'services/shortcuts/src/key_abi.rs';

/** The numbers of the Rust copy: constant name -> header macro. */
const PAIRS = {
  MAGIC: 'KEYS_INFO_MAGIC',
  VERSION: 'KEYS_INFO_VERSION',
  FULL: 'KEYS_FULL',
  DUPLICATE: 'KEYS_DUPLICATE',
};

/** A C number: decimal or 0x hexadecimal, optional parentheses and minus sign, a `u` suffix. */
function number(word) {
  const m = /^\(?(-?)(0x[0-9a-f_]+|[0-9_]+)u?\)?$/i.exec(word.trim());
  if (!m) throw new Error(`not a number: ${word}`);
  const v = Number(m[2].replaceAll('_', ''));
  return m[1] ? -v : v;
}

const header = readFileSync(join(root, HEADER), 'utf8');
const rust = readFileSync(join(root, RUST), 'utf8');
const bad = [];
for (const [name, macro] of Object.entries(PAIRS)) {
  const h = new RegExp(`^#define ${macro} (\\S+)`, 'm').exec(header);
  const r = new RegExp(`pub const ${name}: [a-z0-9]+ = ([^;]+);`).exec(rust);
  if (!h) bad.push(`${HEADER}: ${macro} is not defined`);
  else if (!r) bad.push(`${RUST}: ${name} is not defined`);
  else if (number(h[1]) !== number(r[1]))
    bad.push(`${name} is ${r[1]} in ${RUST} but ${macro} is ${h[1]} in ${HEADER}`);
}
if (bad.length) {
  console.error(`key-abi: ${bad.join('\n')}`);
  process.exit(1);
}
