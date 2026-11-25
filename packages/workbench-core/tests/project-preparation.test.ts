import type { SampleEntry, SampleVerified, SamplesBackend } from '@digi/dk-session/samples';
import { describe, expect, it, vi } from 'vitest';
import { delayPanelSteps, stageProbeSample } from '../src/reference/project-preparation.ts';

const folder: SampleEntry = { name: 'DKREF-75158c5caea8', kind: 'folder', size: 0 };
const probe: SampleEntry = { name: '01-delay-probe', kind: 'file', size: 20 };

function fakeSamples(existing?: SampleEntry, verified = true) {
  const bytes = new Uint8Array(20);
  const root = existing ? [folder] : [];
  const backend = {
    prepare: () => ({ name: probe.name, bytes, stored: bytes, kind: 'converted' as const }),
    list: vi.fn(async (path?: string) => (path ? (existing ? [existing] : []) : root)),
    mkdir: vi.fn(async () => {}),
    put: vi.fn(async () => {}),
    verify: vi.fn(async (): Promise<SampleVerified> => ({
      ok: verified,
      device: verified ? '12345678' : 'ffffffff',
      host: '12345678',
      size: 20,
      sent: 20,
      mismatchAt: null,
      compared: 'content hash',
      message: verified ? 'matches' : 'differs',
    })),
  } satisfies Pick<SamplesBackend, 'prepare' | 'list' | 'mkdir' | 'put' | 'verify'>;
  return backend;
}

describe('reference project preparation', () => {
  it('creates the probe folder, uploads a new file and checks the stored hash', async () => {
    const samples = fakeSamples();
    const placed = await stageProbeSample(samples, new Uint8Array(1));
    expect(placed).toMatchObject({
      destination: `+Drive/${folder.name}/${probe.name}`,
      reused: false,
      deviceHash: '12345678',
    });
    expect(samples.mkdir).toHaveBeenCalledOnce();
    expect(samples.put).toHaveBeenCalledOnce();
    expect(samples.verify).toHaveBeenCalledOnce();
  });

  it('reuses a matching file without writing and never replaces a mismatch', async () => {
    const matching = fakeSamples(probe);
    expect((await stageProbeSample(matching, new Uint8Array(1))).reused).toBe(true);
    expect(matching.mkdir).not.toHaveBeenCalled();
    expect(matching.put).not.toHaveBeenCalled();

    const different = fakeSamples(probe, false);
    await expect(stageProbeSample(different, new Uint8Array(1))).rejects.toThrow('It was not replaced');
    expect(different.put).not.toHaveBeenCalled();
  });

  it('refuses to report a successful upload when verification fails', async () => {
    const samples = fakeSamples(undefined, false);
    await expect(stageProbeSample(samples, new Uint8Array(1))).rejects.toThrow('Could not verify');
  });

  it('keeps hardware assignment and shared Delay settings explicit', () => {
    const staged = '+Drive/folder/probe';
    expect(delayPanelSteps('unassigned', 'Scratch', staged).join(' ')).toContain(`add ${staged}`);
    const seeded = delayPanelSteps('seeded', 'Scratch', staged).join(' ');
    expect(seeded).not.toContain(`add ${staged}`);
    expect(seeded).toContain('FDBK 32');
  });
});
