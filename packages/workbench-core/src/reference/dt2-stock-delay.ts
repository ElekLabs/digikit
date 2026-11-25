export const delayReference = {
  id: 'dt2-stock-delay',
  port: 'dt2-1.17',
  rate: 48_000,
  fixture: '01-delay-probe.wav',
  fixtureSha256: '75158c5caea8e154394dc684e29b06e0c62acfc0f2a823b7cdc1a1636a6e4c6f',
  send: 111,
  spacingMs: [58, 68],
  lossDb: [4.5, 7.5],
  pairSpacingMs: 2,
  pairLossDb: 1,
  project: {
    name: 'Delay probe',
    tempoBpm: 120,
    sampleAssignment: { track: 1, machine: 'One Shot' },
    delaySettings: { timeMs: 64, feedback: 32, volume: 110, initialSend: 0 },
  },
} as const;

export interface DelayTarget {
  setDelaySend(value: number): Promise<void>;
  wait(ms: number): Promise<void>;
  tap(key: string, holdMs: number): Promise<void>;
}

export type DelayTake = 'dry' | 'delay';
type Capture<T> = (take: DelayTake, send: number, play: () => Promise<void>) => Promise<T>;

/** The same stock effect case runs on either target. The caller prepares the project and supplies its recorder. */
export async function captureStockDelay<T>(target: DelayTarget, capture: Capture<T>): Promise<Record<DelayTake, T>> {
  const setSend = async (value: number) => {
    await target.setDelaySend(value);
    await target.wait(300);
  };
  const take = async (kind: DelayTake, send: number) => {
    await setSend(send);
    return capture(kind, send, async () => {
      await target.wait(180);
      await target.tap('TRIG 1', 150);
      await target.wait(1450);
    });
  };
  const drySend = delayReference.project.delaySettings.initialSend;
  try {
    return { dry: await take('dry', drySend), delay: await take('delay', delayReference.send) };
  } finally {
    await setSend(drySend);
  }
}
