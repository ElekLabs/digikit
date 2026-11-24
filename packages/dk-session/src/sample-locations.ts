import type { DkInfoFull } from './dkupload-core.ts';
import type { SamplesKind } from './samples.ts';

const CARD_TOO_SMALL = 5; // INFO card_state

export interface SampleLocation {
  kind: SamplesKind;
  label: string;
  apply: 'live' | 'restart';
  description?: string;
  readOnly?: boolean;
  unavailable?: string;
}

/** Resolve the profile's locations against the runtime on this connection. */
export function sampleLocations(
  kinds: readonly SamplesKind[],
  info: Pick<DkInfoFull, 'card_read_only' | 'card_state'> | null,
  slotApply: SampleLocation['apply'] = 'live',
): SampleLocation[] {
  const hasSlots = kinds.includes('stock-slots');
  return kinds.flatMap((kind): SampleLocation[] => {
    switch (kind) {
      case 'stock-slots':
        return [{ kind, label: 'Sample Slots', apply: slotApply }];
      case 'stock-rpc':
        return [{ kind, label: '+Drive Samples', apply: 'live' }];
      case 'digikit': {
        if (!info && kinds.length > 1) return [];
        const unavailable = !info
          ? 'The digikit runtime is required to access this sample pool.'
          : info.card_state === CARD_TOO_SMALL
            ? 'The card is too small for the digikit store.'
            : undefined;
        return [
          {
            kind,
            label: 'digikit Sample Pool',
            apply: 'live',
            ...(hasSlots ? { description: 'Files are stored here. The stock engine plays from Sample Slots.' } : {}),
            readOnly: info?.card_read_only,
            unavailable,
          },
        ];
      }
    }
  });
}
