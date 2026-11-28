// @vitest-environment happy-dom

import { setImmediate } from 'node:timers/promises';
import { createHash } from 'node:crypto';
import type { SavedSession } from '@digi/workbench-core/host/session.ts';
import { expect, test, vi } from 'vitest';
import { createSignal } from 'solid-js';
import { render } from 'solid-js/web';
import DspPage from '../src/dsp/DspPage';
import ComparisonAudio from '../src/dsp/ComparisonAudio';

const pickFolder = vi.fn<() => Promise<string | null>>();
const read = vi.fn<(_root: string, path: string) => Promise<ArrayBuffer>>();
const encoded = (value: unknown) => new TextEncoder().encode(JSON.stringify(value)).buffer;
const button = (root: Element, label: string) =>
  Array.from(root.querySelectorAll('button')).find((button) => button.textContent === label)!;
const wav = `quality-media/audio-${'a'.repeat(64)}.wav`;
const quality = (module: string) => ({
  schema: 'dk.module-quality/1',
  module,
  scope: 'declarative SHARC rig checks',
  sampleRate: 48000,
  outcome: 'fail',
  files: [],
  diagnostics: ['Example <script>bad</script>'],
  comparisons: [
    {
      file: '../external.json',
      baseline: { file: '../../dry.wav', wav },
      candidate: { file: '../../wet.wav' },
      plots: [],
    },
  ],
});

test('the report remains open when the page hides and late media reads cannot create URLs', async () => {
  const url = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:preview');
  const [active, setActive] = createSignal(true);
  const root = document.createElement('div');
  document.body.append(root);
  let release!: (bytes: ArrayBuffer) => void;
  const delayed = new Promise<ArrayBuffer>((resolve) => (release = resolve));
  pickFolder.mockResolvedValueOnce('/picked').mockResolvedValueOnce(null);
  read.mockImplementation((_root, path) =>
    path.endsWith('.wav') ? delayed : Promise.resolve(encoded(quality('some/fx'))),
  );
  const dispose = render(() => <DspPage active={active()} fs={{ pickFolder, read }} openTransfer={() => {}} />, root);
  root.querySelector<HTMLButtonElement>('button')!.click();
  await vi.waitFor(() => expect(root.textContent).toContain('some/fx'));
  expect(root.querySelector('script')).toBeNull();
  await vi.waitFor(() => expect(read.mock.calls.some(([, path]) => path === wav)).toBe(true));
  setActive(false);
  expect(root.textContent).not.toContain('some/fx');

  const bytes = new Uint8Array(44);
  bytes.set(new TextEncoder().encode('RIFF'));
  bytes.set(new TextEncoder().encode('WAVE'), 8);
  release(bytes.buffer);
  await delayed;
  await setImmediate();
  expect(url).not.toHaveBeenCalled();

  setActive(true);
  expect(root.textContent).toContain('some/fx');
  root.querySelector<HTMLButtonElement>('button')!.click();
  await vi.waitFor(() => expect(root.querySelector<HTMLButtonElement>('button')!.disabled).toBe(false));
  expect(root.textContent).toContain('some/fx');
  dispose();
  root.remove();
  url.mockRestore();
});

test('A/B uses the same playback time and releases both players on disposal', () => {
  const root = document.createElement('div');
  document.body.append(root);
  const position = vi.fn();
  const dispose = render(() => <ComparisonAudio urls={['blob:a', 'blob:b']} position={position} />, root);
  const [left, right] = Array.from(root.querySelectorAll('audio'));
  for (const audio of [left, right]) {
    Object.defineProperty(audio, 'duration', { value: 3 });
    audio.play = vi.fn(async () => {});
    audio.pause = vi.fn();
    audio.load = vi.fn();
    audio.dispatchEvent(new Event('loadedmetadata'));
  }
  const [play, , selectB] = root.querySelectorAll('button');
  play.click();
  left.currentTime = 1.25;
  selectB.click();
  expect(left.pause).toHaveBeenCalledOnce();
  expect(right.currentTime).toBe(1.25);
  expect(right.play).toHaveBeenCalledOnce();
  expect(position).toHaveBeenLastCalledWith(1.25);
  dispose();
  root.remove();
  expect(left.load).toHaveBeenCalledOnce();
  expect(right.load).toHaveBeenCalledOnce();
});

test('the picker refreshes on opening, shows catalog failures, and runs the selected module', async () => {
  const [active, setActive] = createSignal(false);
  const listModules = vi.fn<() => Promise<{ id: string; name: string }[]>>();
  const run = vi.fn().mockResolvedValue({ folder: '/generated' });
  const fs = {
    pickFolder: vi.fn(),
    read: vi.fn().mockResolvedValue(encoded({ ...quality('digikit/ascend'), comparisons: [] })),
  };
  const root = document.createElement('div');
  document.body.append(root);
  listModules.mockResolvedValueOnce([]).mockRejectedValueOnce(new Error('Checkout missing'));
  listModules.mockResolvedValueOnce([{ id: 'digikit/ascend', name: 'Ascend' }]);
  const dispose = render(
    () => <DspPage active={active()} fs={fs} rig={{ listModules, run }} openTransfer={() => {}} />,
    root,
  );
  expect(listModules).not.toHaveBeenCalled();
  setActive(true);
  expect(root.textContent).toContain('Loading rig modules…');
  await vi.waitFor(() => expect(root.textContent).toContain('No machines or effects'));
  expect(button(root, 'Run rig report').disabled).toBe(true);
  setActive(false);
  setActive(true);
  await vi.waitFor(() => expect(root.querySelector('[role="alert"]')?.textContent).toContain('Checkout missing'));
  button(root, 'Refresh modules').click();
  await vi.waitFor(() => expect(root.textContent).toContain('Ascend (digikit/ascend)'));
  const select = root.querySelector('select')!;
  select.value = 'digikit/ascend';
  select.dispatchEvent(new Event('change', { bubbles: true }));
  button(root, 'Run rig report').click();
  await vi.waitFor(() => expect(root.querySelector('h3')?.textContent).toBe('digikit/ascend'));
  expect(run).toHaveBeenCalledWith('digikit/ascend');
  dispose();
  root.remove();
});

test('a session saves a selection before a run and restores its report after a switch', async () => {
  const sessionA = (root: string, module: string, reportFolder?: string): SavedSession => ({
    root,
    document: {
      schema: 'dk.session/1',
      id: root,
      name: 'Session',
      firmware: { path: '/test.syx', sha256: '', device: '', runtime: true, dsp: true, imageSha256: null },
      checkpoint: null,
      attachments: [],
      dsp: { module, reportFolder },
    },
  });
  const old = encoded(quality('old/fx'));
  const moved = encoded(quality('new/fx'));
  const folder = (bytes: ArrayBuffer) => `dsp/${createHash('sha256').update(new Uint8Array(bytes)).digest('hex')}`;
  let release!: (bytes: ArrayBuffer) => void;
  const delayed = new Promise<ArrayBuffer>((resolve) => (release = resolve));
  const fs = {
    pickFolder: vi.fn().mockResolvedValue('/picked'),
    read: vi.fn((root: string, name: string) => {
      if (!name.endsWith('quality.json')) return Promise.reject(new Error('Unavailable'));
      return root === '/old' ? delayed : Promise.resolve(moved);
    }),
  };
  const [session, setSession] = createSignal(sessionA('/selection', 'selected/fx'));
  const saveDsp = vi.fn().mockResolvedValue(undefined);
  const runRig = vi.fn();
  const listModules = vi.fn().mockResolvedValue([{ id: 'another/fx', name: 'Another Effect' }]);
  const root = document.createElement('div');
  document.body.append(root);
  const dispose = render(
    () => (
      <DspPage
        active
        fs={fs}
        session={session()}
        saveDsp={saveDsp}
        rig={{ listModules, run: runRig }}
        openTransfer={() => {}}
      />
    ),
    root,
  );
  const select = root.querySelector('select')!;
  expect(select.value).toBe('selected/fx');
  const save = button(root, 'Save to session');
  save.click();
  await vi.waitFor(() => expect(root.textContent).toContain('DSP selection saved to session.'));
  expect(saveDsp).toHaveBeenCalledWith('selected/fx', undefined);
  setSession(sessionA('/old', 'old/fx', folder(old)));
  await vi.waitFor(() =>
    expect(fs.read).toHaveBeenCalledWith('/old', `${folder(old)}/quality.json`, expect.any(Number)),
  );
  setSession(sessionA('/moved', 'new/fx', folder(moved)));
  await vi.waitFor(() => expect(root.querySelector('h3')?.textContent).toBe('new/fx'));
  expect(select.value).toBe('new/fx');
  expect(select.selectedOptions[0]?.textContent).toContain('(unavailable)');
  expect(button(root, 'Run rig report').disabled).toBe(true);
  expect(fs.read).toHaveBeenCalledWith('/moved', `${folder(moved)}/quality.json`, expect.any(Number));
  release(old);
  await delayed;
  await setImmediate();
  expect(root.querySelector('h3')?.textContent).toBe('new/fx');
  expect(runRig).not.toHaveBeenCalled();
  root.querySelector('button')!.click();
  await vi.waitFor(() => expect(fs.read).toHaveBeenCalledWith('/picked', 'quality.json', expect.any(Number)));
  await vi.waitFor(() => expect(save.disabled).toBe(false));
  save.click();
  await vi.waitFor(() =>
    expect(saveDsp).toHaveBeenLastCalledWith('new/fx', { folder: '/picked', sha256: folder(moved).slice(4) }),
  );
  await vi.waitFor(() => expect(save.disabled).toBe(false));
  select.value = 'another/fx';
  select.dispatchEvent(new Event('change', { bubbles: true }));
  expect(root.textContent).toContain(
    'The open report is for new/fx. Saving another/fx stores the module selection only.',
  );
  save.click();
  await vi.waitFor(() => expect(saveDsp).toHaveBeenLastCalledWith('another/fx', undefined));
  dispose();
  root.remove();
});
