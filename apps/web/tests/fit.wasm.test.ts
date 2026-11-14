// The fit rule through patchkit.wasm (`pk_fit`, packages/patchkit/src/fit.rs): a round trip over a few shared vectors of modules/core/runtime/vectors/fit-vectors.json.
// The rule itself is tested once, in Rust (modules/core/runtime/fit). Skips when patchkit.wasm is not built.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { DkNeeds } from '@digi/workbench-core/modules/dkneeds.ts';
import type { Caps, Needs } from '@digi/workbench-core/modules/fit.ts';
import { WASM } from './fit-wasm.ts';
import { readBytes } from './support/patchkit-env.ts';
import { test } from 'vitest';

type Case = { name: string; caps: Record<string, number[] | null>; needs: Needs; want: [string, number, number] };
type SetCase = {
  name: string;
  caps: Record<string, number[] | null>;
  needs: Needs[];
  want: [number, string, number, number];
};
const V: { caps_base: Record<string, number[]>; cases: Case[]; sets: SetCase[] } = JSON.parse(
  readFileSync(new URL('../../../modules/core/runtime/vectors/fit-vectors.json', import.meta.url), 'utf8'),
);
const capsOf = (over?: Record<string, number[] | null>): Caps =>
  Object.fromEntries(
    Object.entries({ ...V.caps_base, ...over })
      .filter((e): e is [string, number[]] => e[1] !== null)
      .map(([k, [limit, used, largest]]) => [k, { limit: limit!, used: used!, largest: largest! }]),
  );
const skip = !existsSync(WASM);

test.skipIf(skip)('each need is judged against the capacity, with the texts of the vectors', async () => {
  const reader = await DkNeeds.create(readBytes(fileURLToPath(WASM)));
  for (const name of [
    'one byte over the largest free range',
    'a file larger than the staging buffer',
    'machine rows',
  ]) {
    const c = V.cases.find((x) => x.name === name)!;
    const got = reader.fit({ caps: capsOf(c.caps), needs: [c.needs] }).each[0]!;
    assert.deepEqual([got.resource, got.need, got.free], c.want, name);
  }
  const fits = V.cases.find((x) => x.name === 'a small ColdFire module')!;
  assert.equal(reader.fit({ caps: capsOf(), needs: [fits.needs] }).each[0], null);
});

test.skipIf(skip)('a set loads in turn and `empty` starts from the limits', async () => {
  const reader = await DkNeeds.create(readBytes(fileURLToPath(WASM)));
  const s = V.sets.find((x) => x.name.startsWith('two pluck-like'))!;
  const got = reader.fit({ caps: capsOf(s.caps), needs: s.needs }).set!;
  assert.deepEqual([got.index, got.resource, got.need, got.free], s.want);
  const used = { ...capsOf(), MODULES: { limit: 8, used: 8, largest: 0 } };
  const n = [{ ARENA: 48 }];
  assert.equal(reader.fit({ caps: used, needs: n }).set?.resource, 'MODULES');
  assert.equal(reader.fit({ caps: used, needs: n, empty: true }).set, null);
});

test.skipIf(skip)('a short carries the wordings for a person, and a bad request is refused', async () => {
  const reader = await DkNeeds.create(readBytes(fileURLToPath(WASM)));
  const c = V.cases.find((x) => x.name === 'parameter rows')!;
  const short = reader.fit({ caps: capsOf(c.caps), needs: [c.needs] }).each[0]!;
  assert.deepEqual(
    [short.text, short.brief, short.describe, short.describe_by],
    ['PARAMS 3 > 2 FREE', 'PARAMS 3>2', 'parameter rows 3 needed, 2 free', 'parameter rows 1 short'],
  );
  assert.throws(() => reader.fit({ caps: 1 as never, needs: [] }), /caps/);
});
