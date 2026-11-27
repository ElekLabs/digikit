import type { CardOptions } from './card-store.ts';

/** A card: `modules` are module ids (stored as modules/<last>.dkm) or `{ name, file }` store entries; the rest are the `buildCard` options. */
export type CardSpec = Omit<CardOptions, 'dir' | 'modules'> & { modules?: (string | { name: string; file: string })[] };
