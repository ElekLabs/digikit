import { describe, expect, it } from 'vitest';
import { captureStockDelay, type DelayTarget } from '../src/reference/dt2-stock-delay.ts';

function fakeTarget() {
  const steps: (string | number)[][] = [];
  const target: DelayTarget = {
    async setDelaySend(value) {
      steps.push(['send', value]);
    },
    async wait(ms) {
      steps.push(['wait', ms]);
    },
    async tap(key, ms) {
      steps.push(['tap', key, ms]);
    },
  };
  return { target, steps };
}

describe('DT2 stock Delay case', () => {
  it('records the same trigger at zero and wet send, then leaves Track 1 dry', async () => {
    const { target, steps } = fakeTarget();
    const result = await captureStockDelay(target, async (kind, _send, play) => {
      steps.push(['start', kind]);
      await play();
      steps.push(['stop', kind]);
      return `${kind}.wav`;
    });
    expect(result).toEqual({ dry: 'dry.wav', delay: 'delay.wav' });
    expect(steps).toEqual([
      ['send', 0],
      ['wait', 300],
      ['start', 'dry'],
      ['wait', 180],
      ['tap', 'TRIG 1', 150],
      ['wait', 1450],
      ['stop', 'dry'],
      ['send', 111],
      ['wait', 300],
      ['start', 'delay'],
      ['wait', 180],
      ['tap', 'TRIG 1', 150],
      ['wait', 1450],
      ['stop', 'delay'],
      ['send', 0],
      ['wait', 300],
    ]);
  });

  it('restores the send even when the wet recorder fails', async () => {
    const { target, steps } = fakeTarget();
    await expect(
      captureStockDelay(target, async (kind, _send, play) => {
        if (kind === 'delay') throw new Error('capture failed');
        await play();
      }),
    ).rejects.toThrow('capture failed');
    expect(steps.filter(([step]) => step === 'send')).toEqual([
      ['send', 0],
      ['send', 111],
      ['send', 0],
    ]);
  });
});
