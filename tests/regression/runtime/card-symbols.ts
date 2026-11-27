import { RING_SYMBOLS } from './runtime-menu.ts';

/** Runtime variables the card scenarios read or poke (`DKRT_BOOT` and the host directory `DKRT_HOST_DIR`, which holds the addresses of the package's statics, are always planned). */
export const CARD_SYMBOLS = ['DKRT_CARD', 'DKRT_CRASH', 'DKRT_MAILBOX', 'DKRT_STATE', ...RING_SYMBOLS];
