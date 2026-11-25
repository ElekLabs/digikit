import { RuntimeEvents, resolveParameterWatch, withParameterObserver, type DkSession } from '@digi/dk-session';
import type { DeviceRecord } from '@digi/devices';

/** Record actual live values before a capture. A slot number does not prove the sample's identity. */
export async function readStockDelayState(session: DkSession, profile: DeviceRecord, version: string) {
  const track = 1;
  const slot = resolveParameterWatch(profile, version, 'sampleSlot', { scope: 'track', track });
  const feedback = resolveParameterWatch(profile, version, 'delay.feedback', { scope: 'kit' });
  const events = new RuntimeEvents(session, session.clock);
  const target = `${profile.name} OS ${version}`;
  const sampleSlot = await withParameterObserver(session, events, slot, async (value) => value, { target });
  const feedbackRaw = await withParameterObserver(session, events, feedback, async (value) => value, { target });
  return { track, sample_slot: sampleSlot, delay_feedback_raw: feedbackRaw };
}
