// The fake world of the UI preview: a device (modules and a samples tree), a samples folder on the computer, and how fast and how well the
// device answers. The state can be set from the address (`?dev=empty&folder=files&speed=1.5`, see PARAMS) and while the page runs
// (`window.__sim.set({ dev: 'noreply' })`), so a screenshot script (tools/devtools/ui_shots.mjs) reaches each state without clicking.
import { previewHome } from './preview-home';
import type { SampleEntry } from '@digi/dk-session/samples';
import { noteActivity } from '@digi/workbench-core/transfer/activity.ts';
import type { TransferApp } from '@digi/workbench-core/transfer/app.ts';
import type { LocalEntry } from '@digi/workbench-core/transfer/samples-core.ts';

/** The app the preview mocks talk to (main.tsx sets it right after building it): their connection indicator, job and page state are the app's. */
export const simApp: { app?: TransferApp } = {};
export const bindSimApp = (app: TransferApp): void => {
  simApp.app = app;
};

/** dev: ok (answers), empty (nothing stored), noreply (never answers: the wait notice), error (every read fails), slow (2.5 s per read).
 *  folder: files (the samples folder of ~20 WAVs), empty, none (no folder chosen yet). speed: MiB/s of an upload. */
export interface SimState {
  dev: 'ok' | 'empty' | 'noreply' | 'error' | 'slow';
  folder: 'files' | 'empty' | 'none';
  speed: number;
  /** Milliseconds spent verifying each sample (verify=200). */
  verify: number;
  paused: boolean;
  mode: 'modules' | 'samples';
  /** The device storage can make folders (mkdir=0: it cannot). */
  mkdir: boolean;
  /** The second file of an upload cannot be sent (putfail=1). */
  putfail: boolean;
  /** Names (comma separated) of files that the fake device holds already with the same bytes: an upload skips them (skip=kick,snare). */
  skip: string;
  /** The link of the samples: none (no pill), bulk, sysex, wrongmode (SysEx, device in USB AUDIO/MIDI), fallback (bulk until the second file, then SysEx), reason (SysEx with the reason of the bulk choice). */
  link: 'none' | 'bulk' | 'sysex' | 'wrongmode' | 'fallback' | 'reason';
}

export const PARAMS = [
  'dev',
  'folder',
  'speed',
  'verify',
  'paused',
  'mode',
  'mkdir',
  'putfail',
  'skip',
  'link',
] as const;

const query = new URLSearchParams(location.search);
export const sim: SimState = {
  dev: (query.get('dev') as SimState['dev']) ?? 'ok',
  folder: (query.get('folder') as SimState['folder']) ?? 'files',
  speed: Number(query.get('speed') ?? 3),
  verify: Number(query.get('verify') ?? 200),
  paused: query.get('paused') === '1',
  mode: (query.get('mode') as SimState['mode']) ?? 'modules',
  mkdir: query.get('mkdir') !== '0',
  putfail: query.get('putfail') === '1',
  skip: query.get('skip') ?? '',
  link: (query.get('link') as SimState['link']) ?? 'none',
};

export const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** What a read of the device costs and whether it works: the states of the wait notice and the error line. */
export async function deviceRead(): Promise<void> {
  switch (sim.dev) {
    case 'noreply': {
      // a real session over the mocked emulator capture, which never answers: the request waits, Cancel works, a timeout follows
      await (await simApp.app!.target.session()).info();
      return;
    }
    case 'error':
      throw new Error('The card is not mounted (preview: dev=error).');
    case 'slow':
      // slow but alive: the device keeps answering in small pieces, so nothing says "Waiting for ..."
      for (let i = 0; i < 12; i++) {
        await delay(200);
        noteActivity();
      }
      return;
    default:
      return delay(120);
  }
}

const KiB = 1024;
const MiB = 1024 * KiB;

// ---- samples ----------------------------------------------------------------------------------------------------------------------

export const SAMPLES_ROOT = `${previewHome}/Music/Samples`;
const WAVS = [
  ['kick_808_long_tail', 2.4],
  ['kick_punchy', 1.1],
  ['snare_tight_01', 1.6],
  ['snare_rimshot', 1.3],
  ['hat_closed_a', 0.6],
  ['hat_open_wide', 1.8],
  ['clap_stack', 1.2],
  ['tom_low', 2.2],
  ['perc_conga_hi', 1.5],
  ['perc_shaker_loop_120bpm', 3.9],
  ['bass_sub_C1', 4.2],
  ['bass_reese_growl_with_a_very_long_name_to_force_the_ellipsis_in_the_row_120bpm_Fm', 5.7],
  ['lead_pluck_C3', 2.8],
  ['pad_warm_swell', 6.1],
  ['fx_riser_8bars', 5.2],
  ['fx_impact_big', 3.3],
  ['vox_chop_01', 1.9],
  ['vox_chop_02', 2.1],
  ['loop_amen_break_170', 4.6],
  ['loop_dub_chords_90', 5.9],
] as const;

type Kind = 'converted' | 'native' | 'unchanged';
interface LocalFile {
  name: string;
  size: number;
  kind: Kind;
}
const wavs = (names: string[], kb: number): LocalFile[] =>
  names.map((n) => ({ name: `${n}.wav`, size: Math.round(kb * KiB), kind: 'converted' as Kind }));
/** The folders of the chosen folder: each path holds its files; its subfolders are the paths one level below it ('' is the top). */
const dirs = new Map<string, LocalFile[]>([
  [
    '',
    [
      ...WAVS.map(([n, mb]) => ({ name: `${n}.wav`, size: Math.round(mb * MiB), kind: 'converted' as Kind })),
      { name: 'amen_slices.dksmp', size: Math.round(2.0 * MiB), kind: 'native' as Kind },
      { name: 'notes.txt', size: 812, kind: 'unchanged' as Kind },
    ],
  ],
  ['Drums', wavs(['808_kick_a', '808_kick_b', '808_snare', '808_hat'], 900)],
  ['Vocals', wavs(['ooh', 'aah', 'hey'], 1400)],
  ['loops', wavs(['amen_170', 'dub_chords_90', 'funk_guitar_105'], 3200)],
  ['loops/breaks', wavs(['think_break', 'apache_break', 'funky_drummer'], 2600)],
  ['loops/breaks/chopped', wavs(['think_01', 'think_02'], 700)],
  [
    'MIDI',
    [
      { name: 'bassline_a.mid', size: 2 * KiB, kind: 'unchanged' as Kind },
      { name: 'bassline_b.mid', size: 3 * KiB, kind: 'unchanged' as Kind },
      { name: 'drums_swing.mid', size: 5 * KiB, kind: 'unchanged' as Kind },
    ],
  ],
  ['one-shots', wavs(['clap_a', 'clap_b', 'rim', 'cowbell', 'zap'], 180)],
  ['one-shots/vox', wavs(['yeah', 'uh'], 240)],
  [
    'Presets',
    [
      { name: 'kit_a.json', size: 6 * KiB, kind: 'unchanged' as Kind },
      { name: 'kit_b.json', size: 7 * KiB, kind: 'unchanged' as Kind },
    ],
  ],
  ['Presets/empty', []],
]);
let added: LocalFile[] = [];

const levelFiles = (rel: string): LocalFile[] =>
  rel === '' ? [...(dirs.get('') ?? []), ...added] : (dirs.get(rel) ?? []);
const parentDir = (rel: string) => rel.split('/').slice(0, -1).join('/');

/** The `local_list` of the preview's folder at `rel`. */
export function listLocal(rel: string): LocalEntry[] {
  if (sim.folder === 'empty') return [];
  const folders: LocalEntry[] = [...dirs.keys()]
    .filter((d) => d !== '' && parentDir(d) === rel)
    .map((d) => ({ name: d.split('/').pop() as string, kind: 'folder' as const, size: 0 }));
  return [...folders, ...levelFiles(rel).map((f) => ({ name: f.name, kind: 'file' as const, size: f.size }))];
}
export const localKindOf = (rel: string): Kind | undefined => {
  const i = rel.lastIndexOf('/');
  return levelFiles(i < 0 ? '' : rel.slice(0, i)).find((f) => f.name === rel.slice(i + 1))?.kind;
};
export const localSizeOf = (rel: string): number => {
  const i = rel.lastIndexOf('/');
  return levelFiles(i < 0 ? '' : rel.slice(0, i)).find((f) => f.name === rel.slice(i + 1))?.size ?? 0;
};

// the device's sample folders
const tree = new Map<string, SampleEntry[]>();
function resetTree(): void {
  tree.clear();
  if (sim.dev === 'empty') return;
  tree.set('', [
    { name: 'Drums', kind: 'folder', size: 0 },
    { name: 'Loops', kind: 'folder', size: 0 },
    { name: 'init_tone', kind: 'file', size: 482 * KiB },
  ]);
  tree.set('Drums', [
    { name: '808_kick_a', kind: 'file', size: 900 * KiB },
    { name: 'snare_tight_01', kind: 'file', size: 1.6 * MiB },
    { name: 'hat_closed_a', kind: 'file', size: 0.6 * MiB },
  ]);
  tree.set('Loops', [{ name: 'loop_amen_break_170', kind: 'file', size: 4.6 * MiB }]);
}
resetTree();
export const sampleTree = tree;

export let deviceRevision = 0;

// ---- control from outside -----------------------------------------------------------------------------------------------------------

/** What `window.__sim` offers a script. */
export const control = {
  sim,
  /** Change the state; `dev` and `folder` reset what they stand for. */
  set(next: Partial<SimState>): void {
    const wasOffline = sim.dev === 'noreply';
    Object.assign(sim, next);
    if (next.dev) {
      deviceRevision++;
      resetTree();
      const app = simApp.app;
      if (next.dev === 'noreply')
        app?.connection.noteConnection({ type: 'lost', reason: 'Simulated device disconnected.' });
      else if (wasOffline && app && !app.target.libraryOnly()) {
        const binding = app.target.bindSession();
        void binding
          .session()
          .then((session) =>
            app.connection.noteReply(session, () => !app.target.switching() && app.target.target() === binding.target),
          )
          .catch(() => {});
      }
    }
  },
  /** A file appears in the samples folder (the folder poll or the focus read must show it). */
  addLocal(name: string, size = 1.5 * MiB): void {
    added = [...added, { name, size, kind: name.endsWith('.wav') ? 'converted' : 'unchanged' }];
  },
  /** A file of the top level disappears. */
  removeLocal(name: string): void {
    const top = dirs.get('') ?? [];
    const i = top.findIndex((f) => f.name === name);
    if (i >= 0) top.splice(i, 1);
    added = added.filter((f) => f.name !== name);
  },
  counts: { localList: 0, localPoll: 0, deviceRead: 0 },
};
