// One desktop monitor owns the host's capture stream across all target selections.
import { createSignal } from 'solid-js';
import type { AudioInput, RemoteAudioHost } from '@digi/workbench-core/host/host.ts';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';

const normalize = (name: string) =>
  name
    .trim()
    .toLowerCase()
    .replace(/^elektron\s+/, '')
    .replace(/\s+/g, ' ');

/** Only an unambiguous, complete registered model name may start capture automatically. */
export function matchedInput(port: string, inputs: AudioInput[], ports: string[]): AudioInput | undefined {
  const model = normalize(port);
  if (!registry.devices.some((device) => normalize(device.name) === model)) return;
  // A numbered second unit still makes a plain model name ambiguous.
  const words = (name: string) =>
    ' ' +
    normalize(name)
      .replace(/[^a-z0-9]+/g, ' ')
      .trim() +
    ' ';
  const modelWords = words(model);
  const belongs = (name: string) => words(name).includes(modelWords);
  const candidates = inputs.filter((input) => belongs(input.name));
  if (
    ports.includes(port) &&
    ports.filter(belongs).length === 1 &&
    candidates.length === 1 &&
    normalize(candidates[0]!.name) === model
  )
    return candidates[0];
}

interface MonitorState {
  inputs: AudioInput[];
  choice: string;
  output?: string;
  error?: string;
}

export function createRemoteAudio(host: RemoteAudioHost, midiPorts: () => Promise<string[]>) {
  const [state, setState] = createSignal<MonitorState>({ inputs: [], choice: 'auto' });
  const choices = new Map<string, string>();
  let port: string | undefined;
  let revision = 0;
  let active = false;
  let queue = Promise.resolve();
  let checking = false;

  async function stop() {
    if (active) {
      await host.stop();
      active = false;
    }
  }

  // A target switch must wait for the native stream to close. Keep the queue usable
  // if the host refuses the close, and propagate that error to the target switch.
  function release() {
    ++revision;
    const done = queue.then(stop);
    queue = done.catch(() => {});
    return done;
  }

  function select(next?: string, failure?: string) {
    const token = ++revision;
    const same = !!next && port === next;
    port = next;
    const choice = (next && choices.get(next)) || 'auto';
    setState({ inputs: same ? state().inputs : [], choice, error: failure });
    queue = queue.then(async () => {
      try {
        await stop();
        if (!next || token !== revision || failure) return;
        const inputs = await host.inputs();
        if (token !== revision) return;
        setState({ inputs, choice });
        if (choice === 'off') return;
        const input =
          choice === 'auto' ? matchedInput(next, inputs, await midiPorts()) : inputs.find((item) => item.id === choice);
        if (token !== revision) return;
        if (!input) {
          if (choice !== 'auto') setState({ inputs, choice, error: 'Selected audio input is unavailable.' });
          return;
        }
        const output = await host.start(input.id);
        active = true;
        if (token === revision) setState({ inputs, choice, output });
        else await stop();
      } catch (error) {
        if (token === revision) setState((previous) => ({ ...previous, error: String(error) }));
      }
    });
    return queue;
  }

  async function check() {
    if (!state().output || checking) return;
    const token = revision;
    checking = true;
    try {
      const error = await host.status();
      if (error && token === revision) await select(port, error);
    } catch (error) {
      if (token === revision) await select(port, String(error));
    } finally {
      checking = false;
    }
  }

  function choose(value: string) {
    if (port) choices.set(port, value);
    return select(port);
  }

  return { state, select, release, choose, refresh: () => select(port), check };
}
