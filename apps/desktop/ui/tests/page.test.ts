import { describe, expect, it } from 'vitest';
import { pageFor } from '../src/page-core';

describe('page choice', () => {
  it('sends Transfer and Remote back to Emulator when the emulator has no runtime', () => {
    expect(pageFor('transfer', { kind: 'emulator' }, false)).toBe('emulator');
    expect(pageFor('remote', { kind: 'emulator' }, false)).toBe('emulator');
  });
  it('keeps the pages for a device on a MIDI port, whatever the emulator runs', () => {
    expect(pageFor('transfer', { kind: 'midi' }, false)).toBe('transfer');
    expect(pageFor('remote', { kind: 'midi' }, false)).toBe('remote');
  });
  it('keeps every page when the emulator has a runtime, and the other pages always', () => {
    expect(pageFor('transfer', { kind: 'emulator' }, true)).toBe('transfer');
    expect(pageFor('firmware', { kind: 'emulator' }, false)).toBe('firmware');
    expect(pageFor('emulator', { kind: 'midi' }, false)).toBe('emulator');
  });
});
