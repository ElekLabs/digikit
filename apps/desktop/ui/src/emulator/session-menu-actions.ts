import type { SessionHost } from '@digi/workbench-core/host/session.ts';
import { createEffect, onCleanup, onMount, type Accessor } from 'solid-js';
import type { createWorkspaceSession } from '../workspace-session';

/** Bind the native File actions to the visible Emulator pane, not the lifetime of its saved session. */
export function bindSessionMenu(options: {
  host: Pick<SessionHost, 'onAction' | 'setActionsEnabled'>;
  actions: Pick<ReturnType<typeof createWorkspaceSession>, 'newSession' | 'open' | 'save' | 'saveAs'>;
  enabled: Accessor<boolean>;
  reveal(): void;
  report(message: string): void;
}) {
  const { host, actions, enabled, reveal, report } = options;
  const routes = new Map<string, () => void>([
    ['new', actions.newSession],
    ['open', actions.open],
    ['save', actions.save],
    ['save-as', actions.saveAs],
  ]);
  let active = true;
  let unlisten: (() => void) | undefined;
  const reportIfActive = (error: unknown) => {
    if (active) report(String(error));
  };
  onCleanup(() => {
    active = false;
    unlisten?.();
  });
  onMount(() => {
    void host
      .onAction?.((action) => {
        const run = routes.get(action);
        // The native menu update may still be in flight after a target switch.
        if (!active || !enabled() || !run) return;
        reveal();
        run();
      })
      .then((unsubscribe) => {
        if (active) unlisten = unsubscribe;
        else unsubscribe();
      })
      .catch(reportIfActive);
  });
  createEffect(() => {
    void host.setActionsEnabled?.(enabled()).catch(reportIfActive);
  });
}
