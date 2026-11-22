// A stand-in for `@tauri-apps/api/core` in the preview: just enough for the target selector and a real `DkSession` over the emulator's
// USB capture (a device that never answers: the "no reply" state). Everything else the pages ask of Tauri is answered by the two backend mocks.
import { cardCall, pickFolder } from './card-mock';
import type { RuntimeSnapshot } from '@digi/workbench-core/runtime/runtime.ts';
import * as firmware from './firmware-mock';
import { sim } from './sim';
import { previewSymbolImage } from './symbols-mock';

/** The preview is a plain browser, outside the app: the native runtime is not offered. */
export const isTauri = (): boolean => new URLSearchParams(location.search).get('page') === 'run';

const remotePort = 'Elektron Digitone II';
const symbols = isTauri() && new URLSearchParams(location.search).has('symbols');
const symbolsSnapshot: RuntimeSnapshot = {
  polled: true,
  status: {
    device: previewSymbolImage.device,
    version: previewSymbolImage.firmware,
    icount: 0,
    pc: 0,
    ready: true,
    phase: 'ready',
    error: null,
    frame_revision: 0,
    frame_source: null,
    main_ui_reached: true,
    filesystem_verified: true,
    input_ready: true,
    input_pending: 0,
    input_irqs: 0,
  },
};

export async function invoke<T>(cmd: string, args?: any, _options?: { headers?: Record<string, string> }): Promise<T> {
  switch (cmd) {
    case 'emu_launch':
      return { syx: 'Digitakt_II_OS1.17.syx', ...(symbols ? { loaded: true } : {}) } as T;
    case 'emu_startup':
      if (!symbols) throw new Error('No startup firmware selected');
      return symbolsSnapshot as T;
    case 'emu_step':
      if (!symbols) throw new Error('No startup firmware selected');
      return symbolsSnapshot as T;
    case 'emu_stop':
      return undefined as T;
    case 'emu_audio_status':
      return null as T;
    case 'emu_set_source':
    case 'emu_source_info':
      return { name: 'Digitakt_II_OS1.17.syx', path: 'Digitakt_II_OS1.17.syx', device: 'dt2' } as T;
    case 'emu_build_info':
      return {} as T;
    case 'local_pick_folder':
      return (await pickFolder()) as T;
    case 'card_open_folder':
      return undefined as T;
    case 'remote_audio_inputs':
      return [{ id: 'audio-input', name: remotePort }] as T;
    case 'remote_audio_start':
      return 'Preview headphones' as T;
    case 'remote_audio_status':
      return null as T;
    case 'remote_audio_stop':
      return undefined as T;
    case 'midi_ports':
      return (
        firmware.page ? firmware.ports() : isTauri() ? [remotePort] : ['Elektron Digitakt II', 'IAC Driver Bus 1']
      ) as T;
    case 'midi_open':
      return undefined as T;
    case 'midi_close':
      firmware.midi.close();
      return undefined as T;
    case 'midi_cancel':
      firmware.midi.cancel();
      return undefined as T;
    case 'midi_send':
      firmware.midi.send(args as Uint8Array);
      return undefined as T;
    case 'midi_recv':
      return (await firmware.midi.recv(Number(args?.timeoutMs))) as T;
    case 'patchkit_wasm':
      return (await firmware.patchkitWasm()) as T;
    case 'firmware_prepare':
      return (await firmware.prepare(
        args as Uint8Array,
        _options?.headers?.['x-restore'] === '1',
        _options?.headers?.['x-set'] ?? '',
      )) as T;
    case 'firmware_save':
      return firmware.save(
        decodeURIComponent(_options?.headers?.['x-name'] ?? 'firmware.syx'),
        args as Uint8Array,
      ) as T;
    case 'log_save': {
      // the Save button of the Log drawer: keep the text where a script can read it, answer with a path
      const name = decodeURIComponent(_options?.headers?.['x-name'] ?? 'log.log');
      (window as unknown as { __saved: { name: string; text: string }[] }).__saved ??= [];
      (window as unknown as { __saved: { name: string; text: string }[] }).__saved.push({
        name,
        text: new TextDecoder().decode(args as Uint8Array),
      });
      return `/Users/em/Downloads/${name}` as T;
    }
    case 'emu_call': {
      const method = String(args?.method);
      if (symbols && method === 'runtime.maps') {
        const { device, firmware, sha256 } = previewSymbolImage;
        return {
          image_sha256: sha256,
          matches: true,
          ship: JSON.stringify({
            format: 'dk-addrmap',
            version: 1,
            kind: 'ship',
            key: { device, firmware, image_sha256: sha256 },
            entries: [],
          }),
          symbols: {},
        } as T;
      }
      if (method === 'firmware.load' && isTauri()) return new Promise<T>(() => {});
      if (method === 'status') return { mode: sim.paused ? 'paused' : 'running' } as T;
      if (method.startsWith('card.'))
        return (await cardCall(method, (args?.params as Record<string, unknown>) ?? {})) as T;
      return {} as T; // midi.usb.in / midi.usb.out: nothing is ever answered
    }
    default:
      throw new Error(`preview: no mock for the Tauri command ${cmd}`);
  }
}
