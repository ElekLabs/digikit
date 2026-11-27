import { describe, expect, it } from 'vitest';
import { valueView } from '../workbench/project-text';

const bytes = (...values: number[]) => new Uint8Array(values);
const ascii = (text: string) => new TextEncoder().encode(text);

describe('valueView', () => {
  it('shows printable ASCII as text', () => {
    expect(valueView(ascii('triangle 0.8'), 12)).toEqual({ text: 'triangle 0.8', hex: false, truncated: false });
  });

  it('shows anything else as hex pairs, zero and the high bytes included', () => {
    expect(valueView(bytes(0, 0x7f, 0x80, 0xff, 0x0a), 5)).toEqual({ text: '00 7F 80 FF 0A', hex: true, truncated: false });
    expect(valueView(ascii('tab\there'), 8).hex).toBe(true);
  });

  it('treats an empty value as empty text', () => {
    expect(valueView(bytes(), 0)).toEqual({ text: '', hex: false, truncated: false });
  });

  it('marks a value longer than the bytes shown', () => {
    expect(valueView(ascii('x'.repeat(64)), 100)).toMatchObject({ truncated: true, hex: false });
    expect(valueView(ascii('x'.repeat(64)), 64).truncated).toBe(false);
  });
});
