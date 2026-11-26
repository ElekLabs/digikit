import { describe, expect, test, vi } from 'vitest';
import { pausedTrigger } from '../runtime/note-capture.ts';

describe('pausedTrigger', () => {
  test('pauses the run before it reads the frame counter, and leads from that frame', async () => {
    const order: string[] = [];
    let frame = 1000;
    const session = {
      requireDsp: () => {},
      status: vi.fn(async () => {
        order.push('status');
        return { dsp_frames: frame };
      }),
      client: {
        call: vi.fn(async (method: string, params: { mode: string }) => {
          order.push(`${method}:${params.mode}`);
          frame += 500; // the run advanced until the pause took effect
        }),
      },
    };
    const r = await pausedTrigger(session as never, 300);
    expect(order).toEqual(['run.mode:paused', 'status']);
    expect(r).toEqual({ current: 1500, trigger: 1800 });
  });
});
