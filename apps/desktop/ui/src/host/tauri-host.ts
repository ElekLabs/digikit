// The real Host: the only module that imports Tauri's `invoke` (a lint rule keeps it so). Bytes go as the raw request body with the
// small arguments in headers, as the Rust commands take them (`tauri::ipc::Request`); the names and arguments follow the Rust signatures.
import { invoke, isTauri } from '@tauri-apps/api/core';
import { createFirmwareSymbols } from '../firmware-symbols';
import { localStorageKeyValue } from '@digi/workbench-core/storage.ts';
import type { EmuRpc, Host, SymbolImageMap } from '@digi/workbench-core/host/host.ts';

const timeout = (ms: number) => ({ timeoutMs: Math.max(1, Math.round(ms)) });

export function createTauriHost(): Host {
  const rpc: EmuRpc = {
    call: <T>(method: string, params: Record<string, unknown> = {}, session?: number) =>
      invoke<T>('emu_call', { method, params, session: session ?? null }),
  };
  // Commands from a closing drawer and a newly opened one must reach the
  // native observer in issue order, even across component lifetimes.
  let analyzerTail = Promise.resolve();
  function analyzerCall<T>(command: string, args: Record<string, unknown>): Promise<T> {
    const result = analyzerTail.then(() => invoke<T>(command, args));
    analyzerTail = result.then(
      () => {},
      () => {},
    );
    return result;
  }
  return {
    rpc,
    symbols: createFirmwareSymbols(localStorageKeyValue(), {
      pick: (kind) => invoke<string | null>('symbols_pick', { kind }),
      read: (kind, path) => invoke<SymbolImageMap[]>('symbols_read', { kind, path }),
    }),
    analyzer: {
      open: (source) => analyzerCall('analyzer_open', { source }),
      read: (id) => analyzerCall('analyzer_read', { id }),
      close: (id) => analyzerCall('analyzer_close', { id }),
    },
    fs: {
      pickFolder: ({ start, title }) => invoke('local_pick_folder', { start, title }),
      list: (root, rel) => invoke('local_list', { root, rel }),
      read: (root, rel, head) => invoke('local_read', { root, rel, head }),
      saveLog: (bytes, name, kind = 'log') =>
        invoke('log_save', bytes, { headers: { 'x-name': encodeURIComponent(name), 'x-kind': kind } }),
    },
    library: {
      build: (device) => invoke('lib_build', { device }),
      scan: (device) => invoke('lib_scan', { device }),
      read: (device, path) => invoke('lib_read', { device, path }),
      readMaps: (device) => invoke('lib_maps', { device }),
    },
    ports: {
      audio: {
        inputs: () => invoke('remote_audio_inputs'),
        start: (input) => invoke('remote_audio_start', { input }),
        stop: () => invoke('remote_audio_stop'),
        status: () => invoke('remote_audio_status'),
      },
      midi: {
        ports: () => invoke('midi_ports'),
        open: (name) => invoke('midi_open', { name }),
        close: () => invoke('midi_close'),
        send: (message) => invoke('midi_send', message),
        recv: (ms) => invoke('midi_recv', timeout(ms)),
        cancel: () => invoke('midi_cancel'),
      },
      bulk: {
        list: () => invoke('bulk_list'),
        open: (id) => invoke('bulk_open', { id }),
        close: () => invoke('bulk_close'),
        send: (frame) => invoke('bulk_send', frame),
        recv: (ms) => invoke('bulk_recv', timeout(ms)),
        cancel: () => invoke('bulk_cancel'),
        openOutOnly: (id) => invoke('bulk_open_out_only', { id }),
        sendOutOnly: (frame) => invoke('bulk_send_out_only', frame),
        closeOutOnly: () => invoke('bulk_close_out_only'),
      },
    },
    firmware: {
      prepare: (stock, { restore, set }) =>
        invoke('firmware_prepare', stock, { headers: { 'x-restore': restore ? '1' : '0', 'x-set': set } }),
      save: (bytes, name) => invoke('firmware_save', bytes, { headers: { 'x-name': encodeURIComponent(name) } }),
      systemPackage: () => invoke('system_package'),
      systemPackagePlan: (input) => invoke('system_package_plan', { ...input }),
      patchkitWasm: () => invoke('patchkit_wasm'),
      dkprofWasm: () => invoke('dkprof_wasm'),
      sampleWasm: () => invoke('sample_wasm'),
    },
    emu: {
      setSource: (path) => invoke('emu_set_source', { path }),
      pickSyx: (start) => invoke('emu_pick_syx', { start }),
      sourceInfo: () => invoke('emu_source_info'),
      buildInfo: <T>() => invoke<T>('emu_build_info'),
      launch: () => invoke('emu_launch'),
      inApp: () => isTauri(),
      startup: <T>() => invoke<T>('emu_startup'),
      step: <T>(sessionId: number, budget: number) => invoke<T>('emu_step', { sessionId, budget }),
      tap: <T>(sessionId: number, code: number, hold?: number) => invoke<T>('emu_tap', { sessionId, code, hold }),
      stop: (sessionId) => invoke('emu_stop', { sessionId }),
      audioStatus: <T>(sessionId: number) => invoke<T | null>('emu_audio_status', { sessionId }),
      saveDiagnostics: (name, json) => invoke('emu_save_diagnostics', { name, json }),
      stageSyx: (bytes, name) => invoke('emu_stage_syx', bytes, { headers: { 'x-name': encodeURIComponent(name) } }),
      card: {
        reset: () => invoke('emu_card_reset'),
        stage: (name, bytes) => invoke('emu_card_stage', bytes, { headers: { 'x-name': encodeURIComponent(name) } }),
        load: <T>(name: string | null) => invoke<T>('emu_card_load', { name }),
      },
    },
    shell: {
      openCardFolder: () => invoke('card_open_folder'),
    },
  };
}
