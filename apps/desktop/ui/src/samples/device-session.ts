import { DkError, failureModule, hasCap, identifyDevice, type DkSession, type LoadFailure } from '@digi/dk-session';
import type { Storage } from '@digi/devices';
import { Sample, converterOf } from '@digi/dk-session/dksample';
import {
  openSamples,
  sampleLocations,
  samplesUseBulk,
  type SampleLocation,
  type SamplesBackend,
  type SamplesKind,
  type SamplesOptions,
} from '@digi/dk-session/samples';
import { chooseBulk, type BulkChoice } from '@digi/dk-session/bulk-select';
import type { BulkLink } from '@digi/dk-session/bulk';
import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { errorLine, isCancelled } from '@digi/workbench-core/modules/backend.ts';
import type { ConnectionApp } from '@digi/workbench-core/transfer/connection-state.ts';
import type { TransferBulk } from '@digi/workbench-core/transfer/env.ts';
import type { LinkState } from '@digi/workbench-core/transfer/link-core.ts';
import type { TargetBinding } from '@digi/workbench-core/transfer/target.ts';
import type { LogWriter } from '../log-state';

export interface SampleDevice {
  name: string;
  runtime: string | null;
  locations: readonly SampleLocation[];
}

interface OpenedSamples {
  backend: SamplesBackend;
  link: 'bulk' | 'sysex';
}

interface SessionCache extends SampleDevice {
  s: DkSession;
  generation: number;
  storage: Storage;
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
  firmwareGeneration(): number;
  link(): LinkState | undefined;
  setLink(kind: SamplesKind, link: LinkState): void;
}

export type SampleConnection = Pick<ConnectionApp, 'asked' | 'noteConnection'>;

interface Dependencies {
  sampleWasm(): Promise<ArrayBuffer>;
  connection: () => SampleConnection;
  bulk: () => TransferBulk | undefined;
  writer: Pick<LogWriter, 'logEvent'>;
}

export interface SampleDeviceSession {
  identify(): Promise<SampleDevice>;
  open(kind: SamplesKind, purpose: 'read' | 'write' | 'upload'): Promise<SamplesBackend>;
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

  return ({ binding, current, check, firmwareGeneration, link, setLink }: SampleDeviceContext): SampleDeviceSession => {
    let last: SessionCache | undefined;
    let pending = Promise.resolve();
    let bulkWork: Promise<void> | undefined;

    function checkGeneration(generation: number) {
      check();
      if (generation !== firmwareGeneration())
        throw new DkError({ error: 'The emulator firmware changed.', kind: 'cancelled' });
    }

    async function checked<T>(operation: () => Promise<T>, generation = firmwareGeneration()): Promise<T> {
      checkGeneration(generation);
      try {
        const value = await operation();
        checkGeneration(generation);
        return value;
      } catch (e) {
        checkGeneration(generation);
        throw e;
      }
    }

    function request<Args extends unknown[], Result>(f: (...args: Args) => Promise<Result>) {
      const generation = firmwareGeneration();
      return (...args: Args): Promise<Result> => checked(() => f(...args), generation);
    }

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

    async function checkedLink(
      open: () => Promise<BulkLink>,
      choosing: () => Promise<void>,
      generation: number,
    ): Promise<BulkLink> {
      checkGeneration(generation);
      // Once the native slot can be touched, release waits through the probe and its cleanup.
      bulkWork = choosing();
      const link = await open();
      if (!current() || generation !== firmwareGeneration()) {
        try {
          await link.close();
        } finally {
          checkGeneration(generation);
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

    async function updateBulk(cache: SessionCache, useBulk: boolean, forRun: boolean, note: (line: string) => void) {
      const s = cache.s;
      if ((forRun || !useBulk) && cache.bulk) {
        const previous = cache.bulk;
        cache.bulk = undefined;
        bulkWork = close(previous);
        await bulkWork;
        checkGeneration(cache.generation);
      }
      if (cache.bulk || !useBulk) return;
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
                cache.generation,
              ),
            outOnlyCandidates: env.outOnlyCandidates && request(env.outOnlyCandidates.bind(env)),
            openOutOnly:
              env.openOutOnly &&
              ((id, poll) =>
                checkedLink(
                  () => env.openOutOnly!(id, poll),
                  () => choosing,
                  cache.generation,
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
        checkGeneration(cache.generation);
      }
      cache.reason =
        choiceNote && !NOT_LOADED.test(choiceNote) ? choiceNote.replace(/^USB bulk link not used: /, '') : undefined;
      cache.wrongMode =
        !cache.bulk && binding.target.kind === 'midi' ? await checked(() => s.usbFastWrongMode()) : false;
    }

    function linkOf(cache: SessionCache, link: OpenedSamples['link'], useBulk: boolean): LinkState {
      if (!useBulk) return { state: 'sysex' };
      if (link === 'bulk') return { state: 'bulk' };
      if (cache.failure?.usbFast) return { state: 'sysex', failure: cache.failure.usbFast };
      if (cache.wrongMode) return { state: 'sysex', wrongMode: true };
      return cache.reason ? { state: 'sysex', reason: cache.reason } : { state: 'sysex' };
    }

    async function identify(refreshStore?: 'read' | 'write'): Promise<SessionCache> {
      const { asked, noteConnection } = connection();
      const generation = firmwareGeneration();
      const s = await asked(() => checked(binding.session, generation));
      checkGeneration(generation);
      if (last && (last.s !== s || last.generation !== generation)) {
        const old = last;
        last = undefined;
        bulkWork = close(old.bulk);
        await bulkWork;
        checkGeneration(generation);
      }
      if (!last) {
        const who = await asked(() => checked(() => identifyDevice(port(s), registry), generation));
        checkGeneration(generation);
        const device = registry.get(who.id);
        const { storage } = device;
        if (!storage) throw new Error(`the ${device.name} profile has no [storage]`);
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
          generation,
          name: device.name,
          runtime: who.runtime,
          locations: sampleLocations(storage.samples, who.info, storage.slots?.apply),
          storage,
        };
      } else if (refreshStore && last.runtime !== null && last.storage.samples.includes('digikit')) {
        const { samples, slots } = last.storage;
        try {
          const info = await asked(() => checked(() => s.info(true), generation));
          last.locations = sampleLocations(samples, info, slots?.apply);
        } catch (e) {
          checkGeneration(generation);
          if (refreshStore === 'write' || samples.length === 1 || isCancelled(e)) throw e;
          const unavailable = `Could not check the digikit store. ${errorLine(e)}`;
          last.locations = last.locations.map((item) => (item.kind === 'digikit' ? { ...item, unavailable } : item));
        }
      }
      return last;
    }

    async function open(kind: SamplesKind, purpose: 'read' | 'write' | 'upload'): Promise<SamplesBackend> {
      const cache = await identify(kind === 'digikit' && purpose !== 'read' ? 'write' : undefined);
      checkGeneration(cache.generation);
      const { s } = cache;
      const location = cache.locations.find((candidate) => candidate.kind === kind);
      if (!location || location.unavailable)
        throw new Error(location?.unavailable ?? 'The sample location is unavailable.');
      if (purpose !== 'read' && location.readOnly) throw new Error('The digikit store is read-only.');
      const forRun = purpose === 'upload';
      const useBulk = cache.runtime !== null && samplesUseBulk(kind);
      const note = (why: string) => {
        if (!current()) return;
        console.info(`[samples] ${why}`);
        if (cache.logged === why) return;
        cache.logged = why;
        writer.logEvent('transfer', 'info', `samples: ${why}`);
      };
      const had = cache.bulk;
      if (useBulk && !cache.bulk && !cache.failure) {
        const failure = await usbFastFailure({
          info: request((...args) => s.info(...args)),
          loadFailures: request(() => s.loadFailures()),
        });
        checkGeneration(cache.generation);
        cache.failure = { usbFast: failure };
      }

      // A run chooses again because the device expires its idle bulk session. A refresh keeps the current link and fallback pill.
      await updateBulk(cache, useBulk, forRun, note);
      checkGeneration(cache.generation);
      if (cache.opened?.backend.kind === kind && had === cache.bulk) {
        if (forRun || !link()) setLink(kind, linkOf(cache, cache.opened.link, useBulk));
        return cache.opened.backend;
      }

      const conv = await checked(converter, cache.generation);
      checkGeneration(cache.generation);
      const opened: OpenedSamples = {
        backend: openSamples(kind, {
          session: port(s),
          hashOf: (d) => conv.hash(d),
          converter: converterOf(conv),
          slots: cache.storage.slots,
          bulk: cache.bulk?.session,
          note,
          onFallback: (reason) => {
            if (!current() || cache.generation !== firmwareGeneration() || cache.opened !== opened) return;
            writer.logEvent('transfer', 'error', `samples: Fast USB stopped, the rest goes over SysEx: ${reason}`);
            setLink(kind, { state: 'fallback', reason });
          },
        }),
        link: cache.bulk ? 'bulk' : 'sysex',
      };
      cache.opened = opened;
      setLink(kind, linkOf(cache, opened.link, useBulk));
      return opened.backend;
    }

    function serial<T>(task: () => Promise<T>): Promise<T> {
      const opened = pending.then(task);
      pending = opened.then(
        () => {},
        () => {},
      );
      return opened;
    }

    return {
      identify: () =>
        serial(async () => {
          const { name, runtime, locations } = await identify('read');
          return { name, runtime, locations };
        }),
      open: (kind, purpose) => serial(() => open(kind, purpose)),
      async release() {
        // Enumeration and the outgoing nonce cannot claim the slot after the binding has expired.
        await bulkWork?.catch(() => {});
        await close(last?.bulk);
      },
    };
  };
}
