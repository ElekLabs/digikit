import { failureModule, hasCap, identifyDevice, type DkSession, type LoadFailure } from '@digi/dk-session';
import { Sample, converterOf } from '@digi/dk-session/dksample';
import {
  openSamples,
  samplesUseBulk,
  type SamplesBackend,
  type SamplesOptions,
  type SlotsOptions,
} from '@digi/dk-session/samples';
import { chooseBulk, type BulkChoice } from '@digi/dk-session/bulk-select';
import type { BulkLink } from '@digi/dk-session/bulk';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import type { ConnectionApp } from '@digi/workbench-core/transfer/connection-state.ts';
import type { TransferBulk } from '@digi/workbench-core/transfer/env.ts';
import type { LinkState } from '@digi/workbench-core/transfer/link-core.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import type { LogWriter } from '../log-state';

export interface OpenedSamples {
  name: string;
  runtime: string | null;
  hasRuntime: boolean;
  samples: string;
  apply: 'live' | 'restart';
  backend: SamplesBackend;
  link: 'bulk' | 'sysex';
}

interface SessionCache {
  s: DkSession;
  name: string;
  runtime: string | null;
  samples: string;
  slots?: SlotsOptions;
  apply: 'live' | 'restart';
  opened?: OpenedSamples;
  failure?: { usbFast?: LoadFailure };
  bulk?: BulkChoice;
  wrongMode?: boolean;
  reason?: string;
  logged?: string;
}

export interface SampleDeviceContext {
  binding: TargetBinding;
  current(): boolean;
  check(): void;
  setLink(link: LinkState): void;
}

export type SampleConnection = Pick<ConnectionApp, 'asked' | 'noteConnection'>;

interface Dependencies {
  sampleWasm(): Promise<ArrayBuffer>;
  connection: () => SampleConnection;
  bulk: () => TransferBulk | undefined;
  writer: Pick<LogWriter, 'logEvent'>;
}

export interface SampleDeviceSession {
  open(purpose: 'read' | 'write' | 'upload'): Promise<OpenedSamples>;
  release(): Promise<void>;
}

/** Find the recorded usb-fast load failure when the running firmware reports module loads. */
async function usbFastFailure(s: Pick<DkSession, 'info' | 'loadFailures'>): Promise<LoadFailure | undefined> {
  try {
    if (!hasCap(await s.info(true), 'LOADS')) return undefined;
    return (await s.loadFailures()).find((f) => failureModule(f) === 'usb-fast');
  } catch {
    return undefined;
  }
}

const NOT_LOADED = /did not take the nonce/;

async function close(choice: BulkChoice | undefined) {
  try {
    await choice?.link.close();
  } catch {
    // The claim is already gone when the device was unplugged.
  }
}

/** The converter is shared within the window. Each selection owns its session negotiation and claimed USB interface. */
export function createSampleDeviceSessions({ sampleWasm, connection, bulk, writer }: Dependencies) {
  let sample: Promise<Sample> | undefined;
  function converter() {
    sample ??= sampleWasm()
      .then((b) => Sample.create(b))
      .catch((e) => {
        sample = undefined;
        throw e;
      });
    return sample;
  }

  return ({ binding, current, check, setLink }: SampleDeviceContext): SampleDeviceSession => {
    let last: SessionCache | undefined;
    let pending = Promise.resolve();
    let bulkWork: Promise<void> | undefined;

    async function checked<T>(operation: () => Promise<T>): Promise<T> {
      check();
      try {
        const value = await operation();
        check();
        return value;
      } catch (e) {
        check();
        throw e;
      }
    }

    const request =
      <Args extends unknown[], Result>(f: (...args: Args) => Promise<Result>) =>
      (...args: Args): Promise<Result> =>
        checked(() => f(...args));

    function port(s: DkSession): SamplesOptions['session'] {
      return {
        rpcChunk: s.rpcChunk,
        rpc: request((...args) => s.rpc(...args)),
        info: request((...args) => s.info(...args)),
        list: request((...args) => s.list(...args)),
        mkdir: request((...args) => s.mkdir(...args)),
        rename: request((...args) => s.rename(...args)),
        put: request((...args) => s.put(...args)),
        delete: request((...args) => s.delete(...args)),
        verify: request((...args) => s.verify(...args)),
        capacity: request((...args) => s.capacity(...args)),
      };
    }

    async function checkedLink(open: () => Promise<BulkLink>, choosing: () => Promise<void>): Promise<BulkLink> {
      check();
      // Once the native slot can be touched, release waits through the probe and its cleanup.
      bulkWork = choosing();
      const link = await open();
      if (!current()) {
        try {
          await link.close();
        } finally {
          check();
        }
      }
      let closing: Promise<void> | undefined;
      return {
        send: request(link.send.bind(link)),
        recv: request(link.recv.bind(link)),
        close: () => (closing ??= Promise.resolve().then(() => link.close())),
        get closed() {
          return link.closed;
        },
      };
    }

    async function updateBulk(cache: SessionCache, forRun: boolean, note: (line: string) => void) {
      const s = cache.s;
      if (forRun && cache.bulk) {
        const previous = cache.bulk;
        cache.bulk = undefined;
        bulkWork = close(previous);
        await bulkWork;
        check();
      }
      if (cache.bulk || cache.runtime === null || !samplesUseBulk(cache.samples)) return;
      const env = bulk();
      let choiceNote: string | undefined;
      if (env && binding.target.kind !== 'none') {
        const choosing: Promise<void> = chooseBulk(
          {
            usbFastNonce: request((nonce) => s.usbFastNonce(nonce)),
            usbFastReplies: s.usbFastReplies && request(s.usbFastReplies.bind(s)),
          },
          {
            candidates: request(env.candidates.bind(env)),
            open: (id) =>
              checkedLink(
                () => env.open(id),
                () => choosing,
              ),
            outOnlyCandidates: env.outOnlyCandidates && request(env.outOnlyCandidates.bind(env)),
            openOutOnly:
              env.openOutOnly &&
              ((id, poll) =>
                checkedLink(
                  () => env.openOutOnly!(id, poll),
                  () => choosing,
                )),
          },
          (line) => {
            choiceNote = line;
            note(line);
          },
        ).then((choice) => {
          cache.bulk = choice;
        });
        await choosing;
        check();
      }
      cache.reason =
        choiceNote && !NOT_LOADED.test(choiceNote) ? choiceNote.replace(/^USB bulk link not used: /, '') : undefined;
      cache.wrongMode =
        !cache.bulk && binding.target.kind === 'midi' ? await checked(() => s.usbFastWrongMode()) : false;
    }

    function linkOf(cache: SessionCache, link: OpenedSamples['link']): LinkState {
      if (link === 'bulk') return { state: 'bulk' };
      if (cache.failure?.usbFast) return { state: 'sysex', failure: cache.failure.usbFast };
      if (cache.wrongMode) return { state: 'sysex', wrongMode: true };
      return cache.reason ? { state: 'sysex', reason: cache.reason } : { state: 'sysex' };
    }

    async function open(forRun: boolean): Promise<OpenedSamples> {
      const { asked, noteConnection } = connection();
      check();
      const s = await asked(() => checked(binding.session));
      check();
      if (last && last.s !== s) {
        const old = last;
        last = undefined;
        bulkWork = close(old.bulk);
        await bulkWork;
        check();
      }
      if (!last) {
        const who = await asked(() => checked(() => identifyDevice(port(s), registry)));
        check();
        const device = registry.get(who.id);
        if (!device.storage) throw new Error(`the ${device.name} profile has no [storage]`);
        noteConnection({
          type: 'answered',
          device: {
            id: who.id,
            name: device.name,
            ...(who.os ? { os: who.os } : {}),
            ...(who.runtime !== null ? { runtime: who.runtime } : {}),
          },
        });
        last = {
          s,
          name: device.name,
          runtime: who.runtime,
          samples: device.storage.samples,
          slots: device.storage.slots,
          apply: device.storage.slots?.apply ?? 'live',
        };
      }
      const cache = last;
      const note = (why: string) => {
        if (!current()) return;
        console.info(`[samples] ${why}`);
        if (cache.logged === why) return;
        cache.logged = why;
        writer.logEvent('transfer', 'info', `samples: ${why}`);
      };
      const had = cache.bulk;
      const hasRuntime = cache.runtime !== null;
      if (!cache.bulk && !cache.failure) {
        const failure = hasRuntime
          ? await usbFastFailure({
              info: request((...args) => s.info(...args)),
              loadFailures: request(() => s.loadFailures()),
            })
          : undefined;
        check();
        cache.failure = { usbFast: failure };
      }

      // A run chooses again because the device expires its idle bulk session. A refresh keeps the current link and fallback pill.
      await updateBulk(cache, forRun, note);
      check();
      if (cache.opened && had === cache.bulk) {
        if (forRun) setLink(linkOf(cache, cache.opened.link));
        return cache.opened;
      }

      const conv = await checked(converter);
      check();
      const opened: OpenedSamples = {
        name: cache.name,
        runtime: cache.runtime,
        hasRuntime,
        samples: cache.samples,
        apply: cache.apply,
        backend: openSamples(cache.samples, {
          session: port(s),
          hashOf: (d) => conv.hash(d),
          converter: converterOf(conv),
          slots: cache.slots,
          bulk: cache.bulk?.session,
          note,
          onFallback: (reason) => {
            if (!current() || cache.opened !== opened) return;
            writer.logEvent('transfer', 'error', `samples: Fast USB stopped, the rest goes over SysEx: ${reason}`);
            setLink({ state: 'fallback', reason });
          },
        }),
        link: cache.bulk ? 'bulk' : 'sysex',
      };
      cache.opened = opened;
      setLink(linkOf(cache, opened.link));
      return opened;
    }

    return {
      open(purpose) {
        const opening = pending.then(() => open(purpose === 'upload'));
        pending = opening.then(
          () => {},
          () => {},
        );
        return opening;
      },
      async release() {
        // Enumeration and the outgoing nonce cannot claim the slot after the binding has expired.
        await bulkWork?.catch(() => {});
        await close(last?.bulk);
      },
    };
  };
}
