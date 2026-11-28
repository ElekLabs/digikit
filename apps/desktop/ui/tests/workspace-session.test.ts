import type { SavedSession, SessionHost } from '@digi/workbench-core/host/session.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import { createRoot } from 'solid-js';
import { expect, onTestFinished, test, vi } from 'vitest';
import { createWorkspaceSession } from '../src/workspace-session';

function document(root = '/sessions/demo', name = 'Demo'): SavedSession {
  return {
    root,
    document: {
      schema: 'dk.session/1',
      name,
      id: 'fixture',
      firmware: {
        path: '/firmware.syx',
        sha256: 'a'.repeat(64),
        device: 'fixture',
        runtime: true,
        dsp: true,
        imageSha256: null,
      },
      checkpoint: null,
      attachments: [{ name: 'old.profile.json', kind: 'capture', path: 'attachments/legacy.capture.json' }],
    },
  };
}

function setup(options: { loaded?: boolean; confirm?: boolean } = {}) {
  const saved = document();
  const host = {
    pick: vi.fn<SessionHost['pick']>().mockResolvedValue('/chosen'),
    open: vi.fn<SessionHost['open']>().mockResolvedValue(saved),
    create: vi.fn<SessionHost['create']>().mockResolvedValue(saved),
    mount: vi.fn<SessionHost['mount']>().mockResolvedValue([saved, '/private/card.img']),
    release: vi.fn<SessionHost['release']>().mockResolvedValue(undefined),
    save: vi.fn<SessionHost['save']>().mockResolvedValue(saved),
    saveDsp: vi.fn<SessionHost['saveDsp']>().mockResolvedValue(saved),
    preserve: vi.fn<SessionHost['preserve']>().mockResolvedValue(undefined),
    attach: vi.fn<SessionHost['attach']>().mockResolvedValue(saved),
    readCapture: vi.fn<SessionHost['readCapture']>().mockResolvedValue('{}'),
    openAttachment: vi.fn<SessionHost['openAttachment']>().mockResolvedValue(undefined),
  };
  const firmware = {
    source: () => ({ path: '/firmware.syx' }),
    emulatorLoaded: () => options.loaded ?? true,
    bootRuntime: () => true,
    bootDsp: () => true,
    bootSession: vi.fn().mockResolvedValue(true),
    updateSessionBuild: vi.fn(),
    closeSession: vi.fn().mockResolvedValue(undefined),
  };
  const storage = memoryKeyValue();
  const openCapture = vi.fn(async (file: Pick<File, 'name' | 'text'>) => {
    await file.text();
  });
  const confirm = vi.fn().mockReturnValue(options.confirm ?? true);
  const session = createRoot((dispose) => {
    onTestFinished(dispose);
    return createWorkspaceSession({
      host,
      firmware,
      storage,
      openCapture,
      confirm,
      pickFirmware: vi.fn().mockResolvedValue('/firmware.syx'),
      blocked: () => false,
      selectEmulator: async () => {},
      clearCapture: () => {},
    });
  });
  return { saved, session, host, firmware, openCapture, confirm, storage };
}

test('first Save As checkpoints the regular card into Untitled Session without replacing the mount', async () => {
  const { session, host, firmware } = setup();
  expect(session.name()).toBe('Untitled Session');
  await session.saveAs();
  expect(host.create).toHaveBeenCalledWith(expect.objectContaining({ name: 'Untitled Session', source: 'live' }));
  expect(session.saved()).toBeDefined();
  expect(firmware.bootSession).not.toHaveBeenCalled();
  expect(session.card()).toBeUndefined();
});

test('a saved session and its legacy capture open while offline, then Start mounts the card', async () => {
  const { session, saved, openCapture, host, firmware } = setup({ loaded: false });
  await session.open();
  await session.openAttachment(saved.document.attachments[0]!);
  expect(openCapture).toHaveBeenCalledWith(expect.objectContaining({ name: 'old.profile.json' }));
  expect(host.readCapture).toHaveBeenCalledWith(saved.root, 'attachments/legacy.capture.json');
  expect(firmware.bootSession).not.toHaveBeenCalled();
  await session.start();
  await session.save();
  expect(host.save).toHaveBeenLastCalledWith(saved.root, '/private/card.img', saved.document.name);
});

test('a failed save keeps the active session, then Save As redirects future saves to the fork', async () => {
  const { session, host, saved } = setup();
  await session.open();
  await session.start();
  host.save.mockRejectedValueOnce(new Error('disk full'));
  await session.save();
  expect(session.saved()).toEqual(saved);
  expect(session.error()).toBe('disk full');
  const copy = document('/sessions/copy', 'Demo Copy');
  host.create.mockResolvedValueOnce(copy);
  await session.saveAs();
  expect(host.create).toHaveBeenCalledWith(expect.objectContaining({ fork: saved.root, card: '/private/card.img' }));
  await session.save();
  expect(host.save).toHaveBeenLastCalledWith(copy.root, '/private/card.img', copy.document.name);
});

test('closing or reverting a working card respects the discard choice', async () => {
  const { session, firmware, confirm } = setup({ confirm: false });
  await session.open();
  await session.start();
  await session.close();
  await session.revert();
  expect(firmware.closeSession).not.toHaveBeenCalled();
  expect(confirm).toHaveBeenCalledTimes(2);
  expect(session.card()).toBe('/private/card.img');
});

test('saving a DSP selection leaves the working card and the draft name alone', async () => {
  const { session, host, saved, firmware } = setup();
  await session.open();
  await session.start();
  session.setName('Draft name');
  const dsp = { module: 'some/fx', reportFolder: `dsp/${'a'.repeat(64)}` };
  const update = { ...saved, document: { ...saved.document, dsp } };
  host.saveDsp.mockResolvedValueOnce(update);
  const source = { folder: '/picked/report', sha256: 'b'.repeat(64) };
  await session.saveDsp(dsp.module, source);
  expect(host.saveDsp).toHaveBeenCalledWith(saved.root, dsp.module, source);
  expect(host.save).not.toHaveBeenCalled();
  expect(firmware.closeSession).not.toHaveBeenCalled();
  expect(session.saved()?.document.dsp).toEqual(dsp);
  expect(session.name()).toBe('Draft name');
  expect(session.card()).toBe('/private/card.img');

  host.open.mockResolvedValueOnce({ ...update, root: '/moved/demo' });
  await session.open('/moved/demo');
  expect(session.dspSession()?.document.dsp).toEqual(dsp);
  expect(session.dspSession()?.root).toBe('/moved/demo');
});
