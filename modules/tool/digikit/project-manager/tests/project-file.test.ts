import { describe, expect, it } from 'vitest';
import { readProjectFile, type Pm } from '../workbench/project-file';

const ok = <T>(value: T) => Promise.resolve({ ok: true as const, value });
const noWait = () => Promise.resolve();

function service(parts: { get?: object; readFile: (offset: number) => object }) {
  return {
    get: () => ok({ found: true, loading: false, name: 'A', counter: 1, ...parts.get }),
    readFile: (request: { offset: number }) => ok(parts.readFile(request.offset)),
  } as unknown as Pm;
}

describe('reading a project file', () => {
  it('refuses a file whose page is empty before its total instead of returning too few bytes', async () => {
    const pm = service({
      readFile: (offset) =>
        offset === 0
          ? { found: true, total: 10, data: new Uint8Array(4) }
          : { found: true, total: 10, data: new Uint8Array(0) },
    });
    const read = await readProjectFile(pm, 0, () => true, noWait);
    expect(read?.state).toBe('error');
  });

  it('gives up when the device keeps answering loading', async () => {
    let asks = 0;
    const pm = service({ get: { loading: true } });
    const counting = { ...pm, get: () => (asks++, pm.get({ slot: 0 })) } as unknown as Pm;
    const read = await readProjectFile(counting, 0, () => true, noWait);
    expect(read).toEqual({ state: 'error', text: 'The device did not finish reading the file.' });
    expect(asks).toBeLessThanOrEqual(60);
  });

  it('gives up when the file pages keep answering loading', async () => {
    const pm = service({ readFile: () => ({ loading: true }) });
    const read = await readProjectFile(pm, 0, () => true, noWait);
    expect(read?.state).toBe('error');
  });

  it('returns the bytes of a complete file', async () => {
    const pm = service({ readFile: () => ({ found: true, total: 3, data: Uint8Array.of(1, 2, 3) }) });
    const read = await readProjectFile(pm, 0, () => true, noWait);
    expect(read).toMatchObject({ state: 'ok', file: { bytes: Uint8Array.of(1, 2, 3) } });
  });
});
