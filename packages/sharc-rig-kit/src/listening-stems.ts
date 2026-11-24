/** Deterministic listening material. Only one stem goes to the send bus in either scene. */
import { FS } from './analysis.ts';
import { PyRandom } from './prng.ts';

export const LISTENING_SCENES = ['send/percussion', 'send/texture'] as const;
export type ListeningScene = (typeof LISTENING_SCENES)[number];
export const LISTENING_TEMPO = 132;
export const LISTENING_SEED = 0x18f12;
const beat = (60 * FS) / LISTENING_TEMPO;
const tau = 2 * Math.PI;

/** Unsent kick and bass, and an independent hat or stab stem. The returned dry mix includes both. */
export function listeningStems(scene: ListeningScene) {
  const beats = scene === 'send/percussion' ? 8 : 16;
  const frames = Math.ceil((beat * beats) / 32) * 32;
  const backing = new Float32Array(frames);
  const send = new Float32Array(frames);
  const rng = new PyRandom(LISTENING_SEED);
  const add = (target: Float32Array, atBeat: number, seconds: number, sample: (t: number, i: number) => number) => {
    const at = Math.round(atBeat * beat);
    const end = Math.min(Math.round(seconds * FS), frames - at);
    for (let i = 0; i < end; i++) target[at + i]! += sample(i / FS, i);
  };
  const attack = (t: number, seconds: number) => Math.min(1, t / seconds);
  const tail = (t: number, end: number) => Math.min(1, Math.max(0, end - t) / 0.01);
  for (let n = 0; n < beats; n++) {
    add(
      backing,
      n,
      0.25,
      (t) =>
        0.32 *
        Math.sin(tau * (49 * t + 1.65 * (1 - Math.exp(-t / 0.022)))) *
        Math.exp(-t / 0.064) *
        attack(t, 0.001) *
        tail(t, 0.25),
    );
    const bassHz = n % 8 < 6 ? 55 : 49;
    add(
      backing,
      n + 0.47,
      0.18,
      (t) =>
        0.14 *
        (Math.sin(tau * bassHz * t) + 0.2 * Math.sin(tau * bassHz * 2 * t)) *
        attack(t, 0.005) *
        Math.exp(-t / 0.09) *
        tail(t, 0.18),
    );
    if (scene === 'send/percussion') {
      for (const [offset, level] of [
        [0.5, 0.22],
        [0.75, n % 2 ? 0.09 : 0.055],
      ] as const) {
        let previous = 0;
        add(send, n + offset, offset === 0.5 ? 0.09 : 0.055, (t) => {
          const noise = rng.uniform(-1, 1);
          const high = noise - previous;
          previous = noise;
          return level * high * attack(t, 0.001) * Math.exp(-t / 0.019);
        });
      }
    } else if (n % 2 === 0) {
      const base = n % 8 < 4 ? 220 : 196;
      add(
        send,
        n + 0.25,
        0.77,
        (t) =>
          0.13 *
          attack(t, 0.022) *
          tail(t, 0.77) *
          Math.exp(-t / 0.48) *
          (Math.sin(tau * base * t) +
            0.42 * Math.sin(tau * base * 1.5 * t) +
            0.3 * Math.sin(tau * base * 2 * t) +
            0.15 * Math.sin(tau * base * 4 * t)),
      );
    }
  }
  const dry = backing.map((x, frame) => x + send[frame]!);
  return { backing, send, dry, beats };
}
