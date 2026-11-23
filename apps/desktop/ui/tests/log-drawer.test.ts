import { describe, expect, test } from 'vitest';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import { createLogDrawer } from '../src/log-state.ts';

const workbench = (storage = memoryKeyValue()) => createLogDrawer(storage);

describe('Workbench log drawer', () => {
  test('ordinary errors are unread and leave the drawer closed', () => {
    const { drawer, log, logWriter } = workbench();

    logWriter.logEvent('card', 'info', 'Card attached');
    logWriter.logEvent('card', 'error', 'Sync failed');

    expect(drawer.active()).toBeUndefined();
    expect(log.unread()).toBe(1);
  });

  test('opening Log marks earlier entries and new arrivals seen', () => {
    const { drawer, log, logWriter } = workbench();
    logWriter.logEvent('transfer', 'error', 'Upload failed');

    drawer.toggle('Log');
    expect(drawer.active()).toBe('Log');
    expect(log.unread()).toBe(0);
    logWriter.logEvent('transfer', 'error', 'Retry failed');
    drawer.toggle('Log');

    expect(drawer.active()).toBeUndefined();
    expect(log.unread()).toBe(0);
    logWriter.logEvent('transfer', 'error', 'Another upload failed');
    expect(log.unread()).toBe(1);
  });

  test('a qualifying single entry or batch opens Log in place of another tool', () => {
    const { drawer, log, logWriter } = workbench();
    drawer.toggle('Analyzer');

    logWriter.logInput({ source: 'build', level: 'error', text: 'Build failed' });
    expect(drawer.active()).toBe('Log');
    expect(log.unread()).toBe(0);

    drawer.toggle('Performance');
    logWriter.logInputs([
      { source: 'device', level: 'error', text: 'A runtime warning', rt: { origin: 'device', module: '' } },
    ]);
    expect(drawer.active()).toBe('Performance');
    expect(log.unread()).toBe(1);

    logWriter.logInputs([
      { source: 'device', level: 'info', text: 'Started', rt: { origin: 'device', module: '' } },
      { source: 'device', level: 'error', text: 'fault: stopped', rt: { origin: 'device', module: '' } },
    ]);
    expect(drawer.active()).toBe('Log');
    expect(log.unread()).toBe(0);
  });

  test('selecting a tool closes Log and toggling the active tool closes the drawer', () => {
    const { drawer } = workbench();

    drawer.toggle('Log');
    drawer.toggle('Analyzer');
    expect(drawer.active()).toBe('Analyzer');
    drawer.toggle('Analyzer');
    expect(drawer.active()).toBeUndefined();

    drawer.toggle('Log');
    drawer.toggle('Performance');
    expect(drawer.active()).toBe('Performance');
    drawer.toggle('Analyzer');
    expect(drawer.active()).toBe('Analyzer');
    drawer.toggle('Performance');
    drawer.toggle('Performance');
    expect(drawer.active()).toBeUndefined();
  });

  test('workbenches keep separate log, drawer, and Follow state', () => {
    const firstStorage = memoryKeyValue({ 'digikit.logFollow': 'off' });
    const secondStorage = memoryKeyValue();
    const first = workbench(firstStorage);
    const second = workbench(secondStorage);

    expect(first.log.followOn()).toBe(false);
    expect(second.log.followOn()).toBe(true);
    first.log.setFollowOn(true);
    second.log.setFollowOn(false);
    expect(first.log.followOn()).toBe(true);
    expect(second.log.followOn()).toBe(false);
    expect(firstStorage.get('digikit.logFollow')).toBe('on');
    expect(secondStorage.get('digikit.logFollow')).toBe('off');

    first.logWriter.logEvent('card', 'error', 'First window error');
    second.drawer.toggle('Analyzer');
    expect(first.drawer.active()).toBeUndefined();
    expect(first.log.unread()).toBe(1);
    expect(second.log.unread()).toBe(0);
    expect(second.log.entries()).toEqual([]);
    second.logWriter.logEvent('build', 'error', 'Second window error');
    expect(second.drawer.active()).toBe('Log');
    expect(second.log.entries().map((entry) => entry.text)).toEqual(['Second window error']);
    expect(first.drawer.active()).toBeUndefined();
    expect(first.log.unread()).toBe(1);
    expect(first.log.entries().map((entry) => entry.text)).toEqual(['First window error']);
  });
});
