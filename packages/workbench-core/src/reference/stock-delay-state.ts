import { RuntimeEvents, resolveParameterWatch, withParameterObserver, type DkSession } from '@digi/dk-session';
import type { DeviceRecord } from '@digi/devices';
import { delayReference, isReferenceDelayTime } from './dt2-stock-delay.ts';
import { matchesNativeSample, type NativeSampleKey, type dt2StockSampleReader } from './dt2-stock-sample-reader.ts';

/** Record actual live values before a capture. A slot number does not prove the sample's identity. */
export async function readStockDelayState(session: DkSession, profile: DeviceRecord, version: string) {
  const track = 1;
  const slot = resolveParameterWatch(profile, version, 'sampleSlot', { scope: 'track', track });
  const time = resolveParameterWatch(profile, version, 'delay.time', { scope: 'kit' });
  const feedback = resolveParameterWatch(profile, version, 'delay.feedback', { scope: 'kit' });
  const events = new RuntimeEvents(session, session.clock);
  const target = `${profile.name} OS ${version}`;
  const sampleSlot = await withParameterObserver(session, events, slot, async (value) => value, { target });
  const timeRaw = await withParameterObserver(session, events, time, async (value) => value, { target });
  const feedbackRaw = await withParameterObserver(session, events, feedback, async (value) => value, { target });
  return { track, sample_slot: sampleSlot, delay_time_raw: timeRaw, delay_feedback_raw: feedbackRaw };
}

/** Read the DT2 Delay case's live settings and check the loaded sample before recording. */
export async function readStockDelayCaptureState(
  session: DkSession,
  profile: DeviceRecord,
  version: string,
  reader: ReturnType<typeof dt2StockSampleReader>,
  expected: NativeSampleKey,
) {
  const state = await readStockDelayState(session, profile, version);
  const actual = await reader.readLoadedKey(session, state.sample_slot);
  return {
    ...state,
    loaded_sample: actual,
    expected_sample: expected,
    sample_matches: matchesNativeSample(actual, expected),
    expected_delay_time_raw_range: delayReference.project.delaySettings.timeRawRange,
    delay_time_in_range: isReferenceDelayTime(state.delay_time_raw),
  };
}
