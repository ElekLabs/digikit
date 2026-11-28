import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { dkpSpans, kindAt, type Span, type SpanKind } from '../workbench/dkp-spans';

interface Vector {
  name: string;
  hex: string;
  used: number;
  entries: [string, string, string][];
  modules: [string, number][];
}
const vectors = JSON.parse(
  readFileSync(new URL('../../../../../runtime/core/vectors/dkp-vectors.json', import.meta.url), 'utf8'),
) as { valid: Vector[]; invalid: { name: string; why: string; hex: string }[] };

const bytesOf = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (pair) => parseInt(pair, 16));
const text = (bytes: Uint8Array, span: Span) => new TextDecoder().decode(bytes.slice(span.start, span.end));
const of = (spans: Span[], kind: SpanKind) => spans.filter((span) => span.kind === kind);

describe('the project file highlighter against the dkp vectors', () => {
  it('colours the header fields of every valid file, in order and without gaps up to used', () => {
    for (const vector of vectors.valid) {
      const bytes = bytesOf(vector.hex);
      const spans = dkpSpans(bytes);
      expect(spans, vector.name).toBeDefined();
      expect(spans!.slice(0, 9).map((span) => span.kind), vector.name).toEqual([
        'magic', 'version', 'device', 'slot', 'name', 'stamp', 'counter', 'used', 'crc',
      ]);
      let at = 0;
      for (const span of spans!) {
        expect(span.start, vector.name).toBe(at);
        at = span.end;
      }
      expect(at, vector.name).toBe(vector.used);
    }
  });

  it('finds each module id and hash and each owner, key and value of the entries', () => {
    for (const vector of vectors.valid) {
      const bytes = bytesOf(vector.hex);
      const spans = dkpSpans(bytes)!;
      expect(of(spans, 'module-id').map((span) => text(bytes, span).slice(1)), vector.name).toEqual(
        vector.modules.map(([id]) => id),
      );
      expect(of(spans, 'module-hash').map((span) => new DataView(bytes.buffer).getUint32(span.start)), vector.name).toEqual(
        vector.modules.map(([, hash]) => hash),
      );
      expect(of(spans, 'key').map((span) => text(bytes, span).slice(1)), vector.name).toEqual(
        vector.entries.map(([, key]) => key),
      );
      expect(
        of(spans, 'value').map((span) => Array.from(bytes.slice(span.start + 2, span.end)).map((b) => b.toString(16).padStart(2, '0')).join('')),
        vector.name,
      ).toEqual(vector.entries.map(([, , value]) => value));
      const owners = new Set(vector.entries.map(([owner]) => owner));
      expect(new Set(of(spans, 'owner').map((span) => text(bytes, span).slice(1))), vector.name).toEqual(owners);
    }
  });

  it('marks the body of a tag it does not know as unknown', () => {
    const vector = vectors.valid.find((v) => v.name === 'modules_two_owners_and_an_unknown_tag')!;
    const bytes = bytesOf(vector.hex);
    const unknown = of(dkpSpans(bytes)!, 'unknown');
    expect(unknown.map((span) => text(bytes, span))).toEqual(['future']);
  });

  it('shows a file of another version or magic as plain hex', () => {
    for (const vector of vectors.invalid.filter((v) => v.why === 'version' || v.name === 'magic')) {
      expect(dkpSpans(bytesOf(vector.hex)), vector.name).toBeUndefined();
    }
    expect(dkpSpans(new Uint8Array(10))).toBeUndefined();
  });

  it('answers kindAt by byte and ends at the last byte of a file cut short', () => {
    const vector = vectors.valid.find((v) => v.name === 'one_owner')!;
    const bytes = bytesOf(vector.hex);
    const spans = dkpSpans(bytes.slice(0, 50))!;
    expect(kindAt(spans, 0)).toBe('magic');
    expect(kindAt(spans, 38)).toBe('crc');
    expect(kindAt(spans, 42)).toBe('section');
    expect(spans[spans.length - 1].end).toBe(50);
    expect(kindAt(spans, 60)).toBeUndefined();
  });
});
