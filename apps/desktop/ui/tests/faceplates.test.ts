import { describe, expect, it } from 'vitest';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { defaultFaceplate, faceplateIds, panels, runtimeFaceplateIds } from '@digi/workbench-core/data/faceplates.ts';

describe('faceplate choice', () => {
  it('offers only devices with a runtime, and each has a faceplate', () => {
    expect(runtimeFaceplateIds.length).toBeGreaterThan(0);
    for (const id of runtimeFaceplateIds) {
      expect(registry.runtimeIds).toContain(id);
      expect(Object.hasOwn(panels, id)).toBe(true);
    }
    for (const id of faceplateIds) expect(runtimeFaceplateIds.includes(id)).toBe(registry.runtimeIds.includes(id));
  });
  it('starts on a runtime device with a faceplate', () => {
    expect(runtimeFaceplateIds).toContain(defaultFaceplate);
  });
  it('does not take inherited object keys as devices', () => {
    expect(Object.hasOwn(panels, 'toString')).toBe(false);
    expect(Object.hasOwn(panels, 'constructor')).toBe(false);
  });
});
