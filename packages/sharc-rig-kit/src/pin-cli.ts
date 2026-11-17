/**
 * Show or write a PCM pin from a capture (docs/TESTING.md "PCM pins").
 *
 *   node packages/sharc-rig-kit/src/pin-cli.ts show  --wav CAPTURE.wav --pin PIN.json [--window FRAMES]
 *   node packages/sharc-rig-kit/src/pin-cli.ts write --wav CAPTURE.wav --pin PIN.json [--window FRAMES] [--exact] [--tol key=value,...]
 *
 * `show` prints the measured numbers, beside the pin when it exists, and the differences; it exits 1 on a difference or a missing pin.
 * `write` prints the same report and then writes the pin: run it only after the report was approved. `--window` is the number of
 * frames from the onset (a new device pin; an existing pin keeps its own). `--exact` stores the SHA-256 of the analysed words: use it only
 * for a test that proves identical output. `--tol` sets tolerance fields (onset, level_db, band_db, pitch_cents, ...).
 */
import { readFileSync } from 'node:fs';
import { compare, measure, parseFloatWav, readPin, report, writePin, type Tolerance } from './pcmpin.ts';
import { wordsHash } from './analysis.ts';

const [cmd, ...rest] = process.argv.slice(2);
const arg = (name: string): string | undefined => {
  const i = rest.indexOf(`--${name}`);
  return i >= 0 ? rest[i + 1] : undefined;
};
const wav = arg('wav');
const pinFile = arg('pin');
if ((cmd !== 'show' && cmd !== 'write') || !wav || !pinFile) {
  console.error(
    'usage: pin-cli.ts show|write --wav CAPTURE.wav --pin PIN.json [--window FRAMES] [--exact] [--tol key=value,...]',
  );
  process.exit(2);
}
const { rate, channels, samples } = parseFloatWav(readFileSync(wav));
const pin = readPin(pinFile);
const windowArg = arg('window');
const windowFrames = pin ? (pin.version === 2 ? pin.window_frames : null) : windowArg ? Number(windowArg) : null;
const m = measure(samples, { rate, channels, windowFrames, blockRms: pin?.version === 1 });
console.log(pin ? 'expected (the pin) vs measured:' : 'measured (no pin yet):');
console.log(report(m.fp, pin));
const diffs = pin ? compare(pin, m.fp, { sha256: wordsHash(m.window) }) : [];
if (pin)
  console.log(diffs.length ? `differences:\n${diffs.map((d) => `  - ${d}`).join('\n')}` : 'within the tolerances');
if (cmd === 'show') process.exit(pin && diffs.length === 0 ? 0 : 1);
const tolerance: Partial<Tolerance> = {};
for (const kv of (arg('tol') ?? '').split(',').filter(Boolean)) {
  const [k, v] = kv.split('=');
  (tolerance as Record<string, number>)[k!] = Number(v);
}
writePin(pinFile, m.fp, {
  tolerance: Object.keys(tolerance).length ? tolerance : undefined,
  exactSha256: rest.includes('--exact') ? wordsHash(m.window) : undefined,
});
console.log(`wrote ${pinFile}`);
