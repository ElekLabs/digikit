import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** The texts of the `devices/fw/` declaration files (`subsystems.toml` at the top left out), in the order `patchkit::fwgen::fw_texts` reads them. */
function fwTexts(root: string, dir = join(root, 'devices', 'fw'), top = true): string[] {
  const out: string[] = [];
  const byName = (x: { name: string }, y: { name: string }) => (x.name < y.name ? -1 : x.name > y.name ? 1 : 0);
  for (const e of readdirSync(dir, { withFileTypes: true }).sort(byName)) {
    const full = join(dir, e.name);
    if (e.isDirectory()) out.push(...fwTexts(root, full, false));
    else if (e.name.endsWith('.toml') && !(top && e.name === 'subsystems.toml')) out.push(readFileSync(full, 'utf8'));
  }
  return out;
}

/**
 * `{ports, fw}`: what `pk_bind` reads besides the stock image to bind a service file (the Transfer page does it on the user's machine). It holds no module
 * and no firmware: the ports are the hashes and hook sites, `fw` the argument plan of each hook.
 */
export function buildBindBundle(root: string): { ports: string[]; fw: string[] } {
  const ports = readdirSync(join(root, 'devices', 'ports'))
    .filter((f) => f.endsWith('.toml'))
    .sort()
    .map((f) => readFileSync(join(root, 'devices', 'ports', f), 'utf8'));
  return { ports, fw: fwTexts(root) };
}
