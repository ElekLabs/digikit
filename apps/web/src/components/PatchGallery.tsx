// The patch page: drop a stock .syx, patch the digikit runtime into it in a worker (patchkit.wasm), then download the result or boot it in the
// emulator. The patch adds the runtime set only; machines, effects and tools are installed over USB on the Transfer page. The firmware never leaves this tab.
import { For, Show, createSignal, onCleanup, onMount } from 'solid-js';
import type { CheckResult, PatchPlan, VerifyReport } from '../patch/patchkit-core';
import { patchedFileName } from '../patch/patchkit-core';
import { putHandoff } from '../patch/handoff';
import { identify, deviceName, type Identity } from '../patch/identify';
import { runtimeSets } from '../data/runtime-sets';
import { panels } from '../data/faceplates';
import type { DeviceId } from '../data/device-profiles';
import PanelArt from './PanelArt';
import SyxFile from './SyxFile';
import './patch.css';

type PatchRun = {
  name: string;
  version: string;
  ids: string[];
  plan: PatchPlan;
  verify: VerifyReport;
  sha256: string;
  bytes: ArrayBuffer;
  url: string;
  file: string;
  device: string;
};
type Reply = {
  type: string;
  id?: number;
  result?: CheckResult;
  error?: string;
  plan?: PatchPlan;
  verify?: VerifyReport;
  sha256?: string;
  syx?: ArrayBuffer;
};

const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');
/** The name the patched file and the result dialog give to the runtime set. */
const RUNTIME_ID = 'digikit/runtime';
const MAX_BYTES = 16 * 1024 * 1024;
const isDevice = (d: string): d is DeviceId => d === 'dt2' || d === 'dn2';
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

function Glyph(props: { d: string; width?: string }) {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <path
        d={props.d}
        fill="none"
        stroke="currentColor"
        stroke-width={props.width ?? '1.8'}
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  );
}
const ARROW = 'M3 8H13M9 4L13 8L9 12';
const CHECK = 'M3 8.5L6.5 12L13 4.5';
const INFO = 'M8 1.8a6.2 6.2 0 1 0 0 12.4A6.2 6.2 0 0 0 8 1.8ZM8 7.5V11M8 5V5.1';
const DOWNLOAD = 'M8 2.5V10.5M4.5 7L8 10.5L11.5 7M3 13.5H13';
const PLAY = 'M4.5 2.5L12.5 8L4.5 13.5Z';
const CLOSE = 'M4 4L12 12M12 4L4 12';

export default function PatchGallery() {
  const [loadError, setLoadError] = createSignal('');
  const [ready, setReady] = createSignal(false);
  const [file, setFile] = createSignal<{ name: string; bytes: ArrayBuffer }>();
  const [check, setCheck] = createSignal<CheckResult>();
  const [identity, setIdentity] = createSignal<Identity>();
  const [fileError, setFileError] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [patchError, setPatchError] = createSignal('');
  const [result, setResult] = createSignal<PatchRun>();
  const [dialogOpen, setDialogOpen] = createSignal(false);
  const [dragging, setDragging] = createSignal(false);
  let worker: Worker | undefined;
  let nextId = 1;
  const pending = new Map<number, { resolve: (r: Reply) => void; reject: (e: Error) => void }>();
  let resultUrl: string | undefined;
  let fileInput: HTMLInputElement | undefined;
  let closeButton: HTMLButtonElement | undefined;

  const send = (msg: Record<string, unknown>, transfer: Transferable[] = []) =>
    new Promise<Reply>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      worker?.postMessage({ ...msg, id }, transfer);
    });

  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && dialogOpen()) setDialogOpen(false);
  };
  onMount(() => {
    worker = new Worker(new URL('../patch/patch-worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }: MessageEvent<Reply>) => {
      if (data.type === 'ready') {
        setReady(true);
        return;
      }
      const waiter = data.id === undefined ? undefined : pending.get(data.id);
      if (!waiter) {
        if (data.type === 'error') setLoadError(data.error ?? 'The patch tool failed to start.');
        return;
      }
      pending.delete(data.id!);
      if (data.type === 'error') waiter.reject(new Error(data.error));
      else waiter.resolve(data);
    };
    worker.onerror = (event) => setLoadError(`The patch tool failed to start: ${event.message}`);
    worker.postMessage({ type: 'init', wasmUrl: `${base}patchkit.wasm`, bundleUrl: `${base}modules/bundle.json` });
    window.addEventListener('keydown', onKey);
  });
  onCleanup(() => {
    worker?.terminate();
    if (resultUrl) URL.revokeObjectURL(resultUrl);
    window.removeEventListener('keydown', onKey);
  });

  /** The runtime set of the device of the loaded firmware. */
  const runtime = () => {
    const d = check()?.device;
    return d && isDevice(d) ? runtimeSets[d] : [];
  };
  const compat = (id: string) => check()?.modules.find((m) => m.id === id);
  /** Every module of the runtime set works on this firmware (patchkit's read-only check). */
  const runtimeFits = () => runtime().length > 0 && runtime().every((id) => compat(id)?.ok === true);
  const firmwareName = () => {
    const c = check(),
      i = identity();
    if (!c) return '';
    return i ? `${i.deviceName} ${i.version}` : deviceName(c.device);
  };
  const canPatch = () => ready() && !busy() && Boolean(file()) && Boolean(check()?.port) && runtimeFits();

  const loadFile = async (picked?: File) => {
    if (!picked) return;
    setFileError('');
    setResult(undefined);
    setPatchError('');
    setCheck(undefined);
    setIdentity(undefined);
    if (picked.size > MAX_BYTES) {
      setFile(undefined);
      setFileError('That file is too large for a firmware update (the limit is 16 MB).');
      return;
    }
    const bytes = await picked.arrayBuffer();
    setFile({ name: picked.name, bytes });
    try {
      const reply = await send({ type: 'check', firmware: bytes.slice(0) });
      const found = reply.result!;
      setCheck(found);
      setIdentity(identify(found.sha256));
    } catch (error) {
      setFile(undefined);
      setFileError(`This is not an Elektron firmware update file: ${message(error)}`);
    }
  };

  const patch = async () => {
    const f = file(),
      c = check();
    if (!f || !c) return;
    setBusy(true);
    setPatchError('');
    setResult(undefined);
    try {
      const reply = await send({ type: 'apply', firmware: f.bytes.slice(0), modules: runtime() });
      const plan = reply.plan!;
      const syx = reply.syx!;
      if (resultUrl) URL.revokeObjectURL(resultUrl);
      resultUrl = URL.createObjectURL(new Blob([syx], { type: 'application/octet-stream' }));
      const name = identity()?.deviceName ?? deviceName(c.device);
      const version = identity()?.version ?? plan.base.version;
      setResult({
        name,
        version,
        ids: [RUNTIME_ID],
        plan,
        verify: reply.verify!,
        sha256: reply.sha256!,
        bytes: syx,
        url: resultUrl,
        file: patchedFileName(name, version, [RUNTIME_ID]),
        device: c.device,
      });
      setDialogOpen(true);
      queueMicrotask(() => closeButton?.focus());
    } catch (error) {
      setPatchError(message(error));
    } finally {
      setBusy(false);
    }
  };

  const boot = async () => {
    const r = result();
    if (!r) return;
    try {
      await putHandoff({ name: r.file, device: r.device, bytes: r.bytes.slice(0) });
      location.assign(`${base}emulator/?handoff=1`);
    } catch (error) {
      setPatchError(`Could not pass the file to the emulator: ${message(error)}`);
    }
  };

  const barNote = () => {
    if (!file()) return 'Add your firmware in step 1 to patch.';
    if (!check()) return 'Checking your firmware…';
    if (!check()!.port) return 'This firmware version cannot be patched.';
    if (!runtimeFits()) return 'The runtime does not fit this firmware.';
    return `Ready to patch ${firmwareName()}.`;
  };

  return (
    <main class="patch">
      <div class="wrap">
        <header class="page-head">
          <h1>Patch your firmware</h1>
          <p>
            Add the digikit runtime to your own Digitakt II or Digitone II firmware. Your file stays in this browser
            tab; nothing is uploaded.
          </p>
        </header>

        <section class="stage" aria-labelledby="st1">
          <div class="stage-head">
            <div class="stage-title">
              <span class="trigkey small" classList={{ lit: Boolean(check()?.port) }} aria-hidden="true">
                1<i />
              </span>
              <h2 id="st1">Your firmware</h2>
            </div>
          </div>
          <input
            ref={fileInput}
            id="firmware-file"
            class="visually-hidden"
            type="file"
            accept=".syx,application/octet-stream"
            onChange={(e) => {
              void loadFile(e.currentTarget.files?.[0]);
              e.currentTarget.value = '';
            }}
          />
          <Show
            when={file()}
            fallback={
              <div
                class="drop"
                classList={{ dragging: dragging() }}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDragging(true);
                }}
                onDragLeave={() => setDragging(false)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDragging(false);
                  void loadFile(e.dataTransfer?.files[0]);
                }}
              >
                <SyxFile />
                <div class="drop-text">
                  <strong>Drop your stock .syx here</strong>
                  <span>Digitakt II OS 1.17 or Digitone II OS 1.12. Download it from Elektron's support site.</span>
                </div>
                <label class="btn primary" for="firmware-file">
                  Choose file
                </label>
              </div>
            }
          >
            <div class="loaded">
              <Show when={check() && isDevice(check()!.device) ? (check()!.device as DeviceId) : undefined}>
                {(device) => (
                  <div
                    class="loaded-art"
                    aria-hidden="true"
                    style={{ '--accent': panels[device()].accent, '--func-legend': panels[device()].func_legend }}
                  >
                    <PanelArt panel={panels[device()]} />
                  </div>
                )}
              </Show>
              <div class="loaded-text">
                <div class="loaded-name">
                  <Show when={check()} fallback="Checking…">
                    {firmwareName()}
                    <Show
                      when={check()!.port}
                      fallback={
                        <span class="badge warn">
                          <Glyph d={INFO} />
                          Cannot be patched
                        </span>
                      }
                    >
                      <span class="badge ok">
                        <Glyph d={CHECK} width="2" />
                        {identity() ? 'Stock, recognised' : 'Recognised'}
                      </span>
                    </Show>
                  </Show>
                </div>
                <div class="loaded-file">
                  {file()!.name}
                  <Show when={check()}> · SHA-256 {check()!.sha256.slice(0, 12)}…</Show>
                </div>
              </div>
              <label class="btn sm" for="firmware-file">
                Change
              </label>
            </div>
            <Show when={check() && !check()!.port}>
              <p class="callout warn" role="status">
                <Glyph d={INFO} />
                This file is not a stock release digikit supports: use Digitakt II 1.17 or Digitone II 1.12. A file that
                digikit already patched cannot be patched again; start from the stock .syx.
              </p>
            </Show>
          </Show>
          <Show when={fileError()}>
            <p class="callout bad" role="alert">
              {fileError()}
            </p>
          </Show>
        </section>

        <section class="stage" aria-labelledby="st2">
          <div class="stage-head">
            <div class="stage-title">
              <span class="trigkey small" classList={{ lit: runtimeFits() }} aria-hidden="true">
                2<i />
              </span>
              <h2 id="st2">What is added</h2>
            </div>
          </div>
          <p class="callout">
            <Glyph d={INFO} />
            <span>
              The patch adds the digikit runtime: the DIGIKIT menu and the loader for modules. Machines, effects and
              tools are not patched in. After you flash, install them over USB on the{' '}
              <a href={`${base}transfer/`}>Transfer</a> page.
            </span>
          </p>
          <Show when={loadError()}>
            <p class="callout bad" role="alert">
              {loadError()}
            </p>
          </Show>
          <Show when={runtime().length}>
            <p class="runtime-set">
              <span>Runtime modules for {firmwareName()}</span>
              <For each={runtime()}>{(id) => <code>{id}</code>}</For>
            </p>
          </Show>
        </section>

        <section class="safety" aria-labelledby="safe-title">
          <h2 id="safe-title">Before you flash</h2>
          <ul>
            <li>
              <strong>Flashing is at your own risk.</strong> No patched image has been tested on hardware yet. Elektron
              does not support patched firmware: do not report problems to them.
            </li>
            <li>
              <strong>Keep the stock .syx.</strong> You need it to go back.
            </li>
            <li>
              <strong>Recovery:</strong> hold <kbd>FUNC</kbd> while powering on for the startup menu, then{' '}
              <kbd>TRIG 4</kbd> for OS UPGRADE. This path accepts firmware over <strong>MIDI DIN only</strong> (USB MIDI
              is not available there), so have a DIN MIDI interface ready.
            </li>
            <li>
              The patcher never changes the bootstrap, the updater or the language section, and it checks the result
              before it offers the download.
            </li>
          </ul>
        </section>
      </div>

      <div class="patchbar">
        <div class="wrap">
          <span class="trigkey small" classList={{ lit: canPatch() || Boolean(result()) }} aria-hidden="true">
            3<i />
          </span>
          <div class="bar-sel">
            <div class="bar-text">
              <div class="bar-title">digikit runtime</div>
              <div class="bar-note">{barNote()}</div>
            </div>
          </div>
          <Show when={result() && !dialogOpen()}>
            <button type="button" class="btn sm" onClick={() => setDialogOpen(true)}>
              Show result
            </button>
          </Show>
          <button type="button" class="btn primary" disabled={!canPatch()} onClick={() => void patch()}>
            {busy() ? 'Patching…' : 'Patch firmware'}
            <Glyph d={ARROW} width="2" />
          </button>
        </div>
        <Show when={patchError()}>
          <div class="wrap">
            <p class="callout bad" role="alert">
              {patchError()}
            </p>
          </div>
        </Show>
      </div>

      <Show when={dialogOpen() && result()}>
        {(r) => (
          <div
            class="scrim"
            onClick={(e) => {
              if (e.target === e.currentTarget) setDialogOpen(false);
            }}
          >
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="done-title">
              <div class="dialog-top">
                <span class="done-mark">
                  <Glyph d={CHECK} width="2" />
                </span>
                <button
                  ref={closeButton}
                  type="button"
                  class="icon-btn"
                  aria-label="Close"
                  onClick={() => setDialogOpen(false)}
                >
                  <Glyph d={CLOSE} width="2" />
                </button>
              </div>
              <div>
                <h2 id="done-title">Your firmware is ready</h2>
                <p>
                  {r().name} OS {r().version} with the digikit runtime. Patched and checked in this browser.
                </p>
              </div>
              <ul class="checks">
                <For each={r().verify.checks.slice(0, 6)}>
                  {(c) => (
                    <li>
                      <Glyph d={CHECK} width="2" />
                      {c}
                    </li>
                  )}
                </For>
              </ul>
              <div class="dialog-actions">
                <a class="btn primary" href={r().url} download={r().file}>
                  <Glyph d={DOWNLOAD} width="2" />
                  Download .syx
                </a>
                <button type="button" class="btn" onClick={() => void boot()}>
                  <Glyph d={PLAY} />
                  Run in emulator
                </button>
              </div>
              <details>
                <summary>Details</summary>
                <dl>
                  <dt>Output</dt>
                  <dd>
                    {r().file} · {r().bytes.byteLength.toLocaleString()} bytes
                  </dd>
                  <dt>SHA-256</dt>
                  <dd>{r().sha256}</dd>
                  <dt>Modules</dt>
                  <dd>{r().plan.record.modules.join(', ')}</dd>
                  <dt>Changed sections</dt>
                  <dd>
                    {r()
                      .plan.output?.changed_sections.map((s) => `#${s[0]}`)
                      .join(', ') || 'none'}
                  </dd>
                  <dt>All checks</dt>
                  <dd>{r().verify.checks.length} passed</dd>
                </dl>
              </details>
              <p class="dialog-note">
                Before you flash: back up your projects and keep the stock .syx so you can go back.
              </p>
            </div>
          </div>
        )}
      </Show>
    </main>
  );
}
