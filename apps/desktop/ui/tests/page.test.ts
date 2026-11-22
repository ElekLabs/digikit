import { describe, expect, it } from 'vitest';
import { pageFor } from '../src/page-core';

describe('page choice', () => {
  it('sends Transfer back to Run when the emulator has no runtime', () => {
    expect(pageFor('transfer', { kind: 'emulator' }, false)).toBe('run');
    expect(pageFor('run', { kind: 'emulator' }, false)).toBe('run');
  });
  it('keeps the pages for a device on a MIDI port, whatever the emulator runs', () => {
    expect(pageFor('transfer', { kind: 'midi' }, false)).toBe('transfer');
    expect(pageFor('run', { kind: 'midi' }, false)).toBe('run');
  });
  it('keeps every page when the emulator has a runtime, and the other pages always', () => {
    expect(pageFor('transfer', { kind: 'emulator' }, true)).toBe('transfer');
    expect(pageFor('firmware', { kind: 'emulator' }, false)).toBe('firmware');
    expect(pageFor('run', { kind: 'midi' }, false)).toBe('run');
  });
});
