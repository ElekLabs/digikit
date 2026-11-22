import type { Target } from '@digi/workbench-core/transfer/target.ts';

export type Page = 'run' | 'transfer' | 'files' | 'firmware';

export const pageFor = (page: Page, target: Pick<Target, 'kind'>, emulatorHasRuntime: boolean): Page =>
  target.kind === 'emulator' && !emulatorHasRuntime && page === 'transfer' ? 'run' : page;
