// Offline render and compare actions for the Workbench CLI.
import path from 'node:path';
import { DEVICES } from '../../packages/sharc-rig-kit/src/spec.ts';
import { renderCase } from '../../packages/sharc-rig-kit/src/render-case.ts';
import { writeAudioComparison, writeAudioRender } from '../../packages/sharc-rig-kit/src/audio-artifact.ts';

export const audioUsage = [
  'wb module render MODULE --device ID --case GROUP/NAME [--emit wav,json] --out DIR',
  'Listening scenes: --case send/percussion|send/texture --send-amount N [--params-f32 V,...] [--profile-block N]',
  'wb audio compare BASELINE.wav|render.json CANDIDATE.wav|render.json --out DIR',
  'Use --format json for a structured answer on stdout.',
].join('\n');

export async function audioCommand({ cmd, sub, rest, opts, fail }) {
  const format = opts.format ?? 'text';
  if (!['text', 'json'].includes(format)) fail('--format is text or json');
  if (!opts.out) fail(audioUsage);
  const dir = path.resolve(opts.out);
  try {
    if (cmd === 'module' && sub === 'render') {
      const want = opts.case ?? [];
      if (rest.length !== 1 || want.length !== 1 || !DEVICES.includes(opts.device))
        fail(audioUsage + '\nAvailable rig devices: ' + DEVICES.join(', '));
      const emit = new Set((opts.emit ?? 'wav,json').split(',').map((s) => s.trim()));
      if ([...emit].some((name) => name !== 'wav' && name !== 'json')) fail('--emit is wav, json or wav,json');
      let send;
      const amount = opts['send-amount'];
      const block = opts['profile-block'];
      const params = opts['params-f32']?.split(',').map((p) => p.trim()) ?? [];
      if (amount !== undefined) {
        if (!amount.trim() || params.includes('') || block?.trim() === '')
          fail('Send amount, parameter values and profile block must be numbers.');
        send = {
          amount: Number(amount),
          params: params.map(Number),
          profileBlock: block === undefined ? undefined : Number(block),
        };
      } else if (params.length || block !== undefined)
        fail('--params-f32 and --profile-block need a listening send case with --send-amount.');
      const { files, manifest } = writeAudioRender(await renderCase(rest[0], opts.device, want[0], send), dir, emit);
      if (format === 'json') console.log(JSON.stringify({ files, ...manifest }));
      else {
        console.log(
          `${manifest.module} ${manifest.device} ${manifest.case} · ${manifest.tap} · ${manifest.summary.fingerprint.frames} frames`,
        );
        for (const file of Object.values(files)) console.log(file);
      }
      return 0;
    }
    if (sub !== 'compare' || rest.length !== 2) fail(audioUsage);
    const { file, report } = writeAudioComparison(rest[0], rest[1], dir);
    if (format === 'json') console.log(JSON.stringify({ file, ...report }));
    else {
      console.log(file);
      for (const warning of report.warnings) console.log(warning);
      if (report.pcm.mode === 'summary-only') console.log(`Summary only. ${report.pcm.reason}`);
      else console.log(report.pcm.identical ? 'PCM samples are identical.' : 'PCM samples differ.');
    }
    return 0;
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error), 1);
  }
}
