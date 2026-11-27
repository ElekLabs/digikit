import { expect, test } from 'vitest';
import type { ServiceRefusal } from '../src/modules/services.ts';
import { visibleRefusals, type TransferRow, type TransferView } from '../src/transfer/transfer-core.ts';

test('Transfer hides dependency refusals using the test module row metadata', () => {
  const row = (id: string, test: boolean): TransferRow => ({
    id,
    test,
    path: `modules/${id}.dkm`,
    name: id,
    group: 'Tools',
    version: '1.0.0',
    file_size: 100,
    on_card: 'no',
    loaded: false,
  });
  const fixture: ServiceRefusal = { module: 'digikit/fixture', service: 'fx', kind: 'missing' };
  const production: ServiceRefusal = { module: 'test/effect', service: 'fx', kind: 'missing' };
  const view: TransferView = {
    device: '',
    runtime: '',
    free_bytes: 1000,
    loaded: [],
    modules: [row(fixture.module, true), row(production.module, false)],
    card_only: [],
    refusals: [fixture, production],
  };

  expect(visibleRefusals(view, false)).toEqual([production]);
  expect(visibleRefusals(view, true)).toEqual([fixture, production]);
});
