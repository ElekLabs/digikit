import type { TransferRow } from '../modules/dktransfer.ts';

/** What a Transfer row needs to reach the sidebar page of a loaded module. */
export interface ModulePageLinks {
  has(id: string): boolean;
  open(id: string): void;
}

/** The Open action of a loaded module's row (none when the module has no page); it navigates to that page. */
export function pageOpenActions(
  row: Pick<TransferRow, 'id' | 'name' | 'known' | 'loaded'>,
  pages: ModulePageLinks | undefined,
): { label: string; title: string; onClick: () => void }[] {
  const id = row.id ?? (row.known ? row.name : undefined);
  if (!pages || !id || !row.loaded || !pages.has(id)) return [];
  return [{ label: 'Open', title: 'Show the page of this module', onClick: () => pages.open(id) }];
}
