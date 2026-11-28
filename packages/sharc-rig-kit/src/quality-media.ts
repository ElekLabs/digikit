/** Copy the saved comparison evidence without decoding or changing the audio. */
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { crc32 } from 'node:zlib';
import { readEvidence, type ComparisonMedia } from './audio-artifact.ts';
import { validateWave } from './wave.ts';

const directory = 'quality-media';
const pngSignature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const digest = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');

function validatePng(bytes: Buffer): void {
  if (!bytes.subarray(0, 8).equals(pngSignature)) throw new Error('Expected a PNG file.');
  let data = false;
  for (let offset = 8; offset + 12 <= bytes.length;) {
    const size = bytes.readUInt32BE(offset);
    const type = bytes.toString('ascii', offset + 4, offset + 8);
    const crc = offset + 8 + size;
    if (crc + 4 > bytes.length || crc32(bytes.subarray(offset + 4, crc)) !== bytes.readUInt32BE(crc))
      throw new Error(`Truncated or corrupt PNG ${type} chunk.`);
    if (offset === 8 && (type !== 'IHDR' || size !== 13 || !bytes.readUInt32BE(16) || !bytes.readUInt32BE(20)))
      throw new Error('Invalid PNG header.');
    if (type === 'IDAT' && size > 0) data = true;
    offset = crc + 4;
    if (type === 'IEND' && size === 0 && data && offset === bytes.length) return;
  }
  throw new Error('PNG is missing complete image data or an end chunk.');
}

export function qualityMedia(out: string, unavailable: ComparisonMedia['unavailable']): ComparisonMedia {
  return {
    unavailable,
    save(file, { kind, sha256, sibling, bytes: verifiedBytes }) {
      try {
        const realFile = realpathSync(file);
        if (sibling && path.dirname(realFile) !== path.dirname(file))
          throw new Error('WAV reference leaves the manifest directory.');
        if (!statSync(realFile).isFile()) throw new Error('Expected a regular file.');
        const bytes = verifiedBytes ?? readEvidence(realFile);
        if (kind === 'wav') validateWave(bytes);
        if (kind === 'png') validatePng(bytes);
        const actualSha256 = digest(bytes);
        if (sha256 && actualSha256 !== sha256)
          throw new Error(`${kind.toUpperCase()} hash does not match the saved evidence.`);

        const root = realpathSync(out);
        const dest = path.join(root, directory);
        mkdirSync(dest, { recursive: true });
        if (realpathSync(dest) !== dest) throw new Error(`${directory} must be a directory inside the report folder.`);
        const stem =
          kind === 'png' && /^(left|right|waveform)\.png$/.test(path.basename(file))
            ? path.basename(file, '.png')
            : { json: 'comparison', png: 'plot', wav: 'audio' }[kind];
        const name = `${stem}-${actualSha256}.${kind}`;
        const temp = mkdtempSync(path.join(dest, '.write-'));
        try {
          const staged = path.join(temp, name);
          writeFileSync(staged, bytes);
          renameSync(staged, path.join(dest, name));
        } finally {
          rmSync(temp, { recursive: true, force: true });
        }
        return `${directory}/${name}`;
      } catch (error) {
        const reason = error instanceof Error ? error.message : String(error);
        unavailable(`${kind.toUpperCase()} ${file}: ${reason} Restore the saved file or regenerate its comparison.`);
        return undefined;
      }
    },
  };
}
