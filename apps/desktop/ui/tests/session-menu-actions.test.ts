import { createRoot, createSignal } from 'solid-js';
import { expect, onTestFinished, test, vi } from 'vitest';
import { bindSessionMenu } from '../src/emulator/session-menu-actions';

test('native session actions are disabled outside the Emulator pane and ignore a late event', async () => {
  const setActionsEnabled = vi.fn(async (_enabled: boolean) => {});
  const open = vi.fn();
  const reveal = vi.fn();
  const disposeListener = vi.fn();
  let receive: ((action: string) => void) | undefined;
  const [enabled, setEnabled] = createSignal(false);
  const dispose = createRoot((dispose) => {
    bindSessionMenu({
      host: {
        setActionsEnabled,
        onAction: async (handler) => {
          receive = handler;
          return disposeListener;
        },
      },
      actions: { newSession: vi.fn(), open, save: vi.fn(), saveAs: vi.fn() },
      enabled,
      reveal,
      report: vi.fn(),
    });
    return dispose;
  });
  onTestFinished(dispose);
  await Promise.resolve();

  expect(setActionsEnabled).toHaveBeenLastCalledWith(false);
  receive?.('open');
  expect(open).not.toHaveBeenCalled();

  setEnabled(true);
  expect(setActionsEnabled).toHaveBeenLastCalledWith(true);
  receive?.('open');
  expect(reveal).toHaveBeenCalledOnce();
  expect(open).toHaveBeenCalledOnce();

  setEnabled(false);
  receive?.('open');
  expect(setActionsEnabled).toHaveBeenLastCalledWith(false);
  expect(open).toHaveBeenCalledOnce();
  dispose();
  expect(disposeListener).toHaveBeenCalledOnce();
});
