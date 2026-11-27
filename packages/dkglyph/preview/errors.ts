import { el } from './element.ts';
import type { SetPayload } from './state.ts';

/** Panel for glyph files that failed to load, and for set-level check errors. */
export function renderErrors(root: HTMLElement, set: SetPayload | null, fetchError: string): void {
  root.replaceChildren();
  if (fetchError) root.append(el('div', { class: 'msg error' }, fetchError));
  for (const e of set?.errors ?? [])
    root.append(el('div', { class: 'msg error' }, `${e.id}: ${e.message}\n${e.where}`));
}
