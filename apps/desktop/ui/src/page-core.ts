// The pure part of the page choice in Console.tsx. Host tests: apps/desktop/ui/tests/page.test.ts.

export type Page = 'emulator' | 'transfer' | 'files' | 'remote' | 'firmware';

/**
 * The page to show. Transfer and Remote talk to the selected target: a device on a MIDI port answers for itself, so the pages stay.
 * Only the emulator can lack a runtime (its device has no `[digikit]`), and then those pages go back to Emulator.
 */
export const pageFor = (
  page: Page,
  target: { kind: 'emulator' | 'midi' | 'none' },
  emulatorHasRuntime: boolean,
): Page =>
  target.kind === 'emulator' && !emulatorHasRuntime && (page === 'transfer' || page === 'remote') ? 'emulator' : page;
