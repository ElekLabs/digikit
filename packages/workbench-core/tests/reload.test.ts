import { describe, expect, it } from 'vitest';
import { reloadError, reloadLine } from '../src/runtime/reload.ts';

describe('the reload status line', () => {
  it('names what a swap wrote and how long it took', () => {
    expect(reloadLine({ path: 'swap', ms: 1432.6, ranges: 2, bytes: 1444, owners: ['test/ui-demo'] })).toBe(
      'Reload: swapped test/ui-demo, 1444 bytes in 2 ranges (1433 ms)',
    );
  });
  it('says why a boot was needed, in seconds', () => {
    expect(reloadLine({ path: 'boot', reason: 'core/runtime changed at 0x4671a000', seconds: 5.34 })).toBe(
      'Reload: booted the new image, core/runtime changed at 0x4671a000 (5.3 s)',
    );
  });
  it('reports no change and a DSP-only change', () => {
    expect(reloadLine({ path: 'none', ms: 1200 })).toBe('Reload: nothing changed (1200 ms)');
    expect(reloadLine({ path: 'dsp', seconds: 0.2 })).toBe('Reload: DSP only, injected (0.2 s)');
  });
  it('names the modules a reload built', () => {
    expect(
      reloadLine({
        path: 'swap',
        ms: 900,
        ranges: 1,
        bytes: 200,
        owners: ['test/ui-demo'],
        build: {
          built: [{ id: 'test/ui-demo', objects: ['ui_demo.dko.json'], ms: 410 }],
          skipped: ['core/ui'],
          ms: 450,
        },
      }),
    ).toBe('Reload: built test/ui-demo (450 ms); swapped test/ui-demo, 200 bytes in 1 ranges (900 ms)');
  });
  it('shows the daemon error without its method prefix', () => {
    expect(reloadError(new Error('dev.reload: the running image has no patch record'))).toEqual({
      status: 'Reload failed: the running image has no patch record',
      full: '',
    });
    expect(reloadError('a job is running; wait for it before dev.reload').status).toBe(
      'Reload failed: a job is running; wait for it before dev.reload',
    );
  });
  it('keeps the compiler output of a build error for the alert', () => {
    const e = reloadError(
      new Error(
        'dev.reload: build of test/ui-demo failed:\nm68k-elf-gcc failed\n/a/b/ui_demo.c:12:3: error: expected expression',
      ),
    );
    expect(e.status).toBe('Reload failed: build of test/ui-demo failed: ui_demo.c:12:3: error: expected expression');
    expect(e.full).toContain('ui_demo.c:12:3: error');
  });
});
