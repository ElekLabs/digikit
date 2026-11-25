// The one reader of a TOML file under a root.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'smol-toml';

export const read = (root, toml) => parse(readFileSync(join(root, toml), 'utf8'));
