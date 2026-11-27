import type { SamplesBackend } from '@digi/dk-session/samples';
import { delayReference } from './dt2-stock-delay.ts';

/** A verified upload can be reused, but it never makes a hardware project ready by itself. */
export async function stageProbeSample(
  samples: Pick<SamplesBackend, 'prepare' | 'list' | 'mkdir' | 'put' | 'verify'>,
  wav: Uint8Array,
) {
  const prepared = samples.prepare(delayReference.fixture, wav);
  const folder = `DKREF-${delayReference.fixtureSha256.slice(0, 12)}`;
  const directory = (await samples.list('')).find((entry) => entry.name === folder);
  if (directory && directory.kind !== 'folder') throw new Error(`+Drive/${folder} is a file. Nothing was written.`);
  if (!directory) await samples.mkdir(folder);
  const existing = (await samples.list(folder)).find((entry) => entry.name === prepared.name);
  const destination = `+Drive/${folder}/${prepared.name}`;
  if (existing?.kind === 'folder') throw new Error(`${destination} is a folder. Nothing was uploaded.`);
  if (!existing) await samples.put(folder, prepared, undefined, { exists: false, folderReady: true });
  const result = await samples.verify(folder, prepared);
  if (!result.ok)
    throw new Error(
      existing
        ? `${destination} already exists with different content. It was not replaced.`
        : `Could not verify ${destination}: ${result.message}`,
    );
  return {
    destination,
    wavSha256: delayReference.fixtureSha256,
    deviceHash: result.device,
    bytes: result.size,
    reused: Boolean(existing),
  };
}

export function delayPanelSteps(assignment: 'seeded' | 'unassigned', name: string, samplePath: string): string[] {
  const {
    tempoBpm,
    sampleAssignment: { track, machine },
    delaySettings,
  } = delayReference.project;
  return [
    assignment === 'seeded'
      ? `Start the saved Workbench session. Check Track ${track} plays the probe in ${machine}.`
      : `In scratch project “${name}”, add ${samplePath} to the sample pool and assign it to Track ${track} in ${machine}.`,
    `Stop transport. Play the probe from the start at original pitch, with the filter open and no modulation. Set all Track ${track} sends to ${delaySettings.initialSend}.`,
    `Set ${tempoBpm} BPM, stock Delay TIME near ${delaySettings.timeMs} ms (roughly a 1/32 note), FDBK ${delaySettings.feedback}, and VOL ${delaySettings.volume}. Use the default Delay filters, ping-pong off, and Delay-to-Reverb zero.`,
    ...(assignment === 'unassigned' ? [`Set Track ${track} MIDI channel to 1 and allow USB MIDI CC/NRPN input.`] : []),
  ];
}
