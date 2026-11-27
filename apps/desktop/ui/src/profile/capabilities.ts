import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { EmuRpc } from '@digi/workbench-core/host/host.ts';
import { processorsOf } from '@digi/workbench-core/profile/processor.ts';
import { DkError, type DkSession } from '@digi/dk-session';

type Support = true | string;
export interface CaptureCapabilities {
  readonly hasDsp: boolean;
  readonly dspBlocks: Support;
  readonly dspHotspots: Support;
  readonly mainHotspots: Support;
}

const NO_DSP_NOTE = 'No DSP on this device.';
const DSP_OFF = 'The DSP is off in this emulator session.';
const NO_SAMPLER = 'This emulator build does not support DSP hotspot recording.';

type EmulatorCapabilities = { main_pc: boolean; dsp_pc: boolean; dsp_active: boolean };

/** Non-mutating discovery scoped to one target binding. The runtime block channel and emulator samplers are independent. */
export function createCaptureCapabilities(rpc?: EmuRpc) {
  let emulatorProbe: { image: string; result: Promise<EmulatorCapabilities | undefined> } | undefined;
  let blockProbe: { session: Pick<DkSession, 'profileDsp'>; image: string; result: Support } | undefined;

  async function inspectEmulator(image: string) {
    if (rpc && emulatorProbe?.image !== image) {
      const result = rpc.call<EmulatorCapabilities>('profile.capabilities').catch((error) => {
        const legacy =
          (error as { code?: number } | null)?.code === -32601 ||
          /method not found|unknown method/i.test(String(error));
        if (!legacy && emulatorProbe?.result === result) emulatorProbe = undefined;
        return undefined;
      });
      emulatorProbe = { image, result };
    }
    return emulatorProbe?.result;
  }

  function hasDsp(device: string): boolean {
    const record = registry.ids.includes(device) ? registry.get(device) : undefined;
    return !record || processorsOf(record).some(({ role }) => role === 'dsp');
  }

  async function inspect(device: string, session: Pick<DkSession, 'profileDsp'>, image: string, checkBlocks: boolean) {
    const build = await inspectEmulator(image);
    const deviceHasDsp = hasDsp(device);
    const dsp: Support = !deviceHasDsp ? NO_DSP_NOTE : build?.dsp_active === false ? DSP_OFF : true;
    let dspBlocks = dsp;
    if (dspBlocks === true) {
      if (blockProbe?.session === session && blockProbe.image === image) dspBlocks = blockProbe.result;
      else if (checkBlocks) {
        try {
          await session.profileDsp('get');
          blockProbe = { session, image, result: true };
        } catch (error) {
          dspBlocks =
            error instanceof DkError && error.status === 'E_PROTO'
              ? 'This runtime does not support DSP block profiling.'
              : 'Could not check DSP block profiling on this target.';
          if (error instanceof DkError && error.status === 'E_PROTO')
            blockProbe = { session, image, result: dspBlocks };
        }
      }
    }
    const capabilities: CaptureCapabilities = {
      hasDsp: deviceHasDsp,
      dspBlocks,
      dspHotspots:
        dsp !== true
          ? dsp
          : !rpc
            ? 'DSP hotspot recording requires the emulator.'
            : build?.dsp_pc === false
              ? NO_SAMPLER
              : true,
      mainHotspots: !rpc
        ? 'Main CPU hotspot recording requires the emulator.'
        : build?.main_pc === false
          ? 'This emulator build does not support main CPU hotspots.'
          : true,
    };
    return capabilities;
  }

  return { inspect };
}
