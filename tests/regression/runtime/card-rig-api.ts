import type { DkSession } from '@digi/dk-session';
import type { InitialButton } from '@digi/control-client';
import type { Device, Session } from '@digi/test-support';
import type { bootInfo } from './guest-mailbox.ts';
import type { cardView, storeList } from './card-store.ts';
import type { SelRecord } from './card-record.ts';
import type { CardSpec } from './card-spec.ts';
import type { uploadStore } from './device-data.ts';
import type { PatchedSession, RuntimeCase, TestRuntime } from '../test-runtime.ts';
import type { PreparedRuntimePlan, RuntimePlan } from './runtime-plan.ts';
import type { runtimeSafety } from './write-safety.ts';

export interface CardRigOptions {
  devices?: readonly Device[];
  /** Full tier only. */
  full?: boolean;
  /** Skip with this reason (the Python scenario still covers it); see the scenario-migration handover. */
  skip?: Partial<Record<Device, string>>;
  /** Module roots of the runtime image (`patchkit apply --modules`); omit for the emulator patch set. */
  roots?: string[];
  /** The image has no runtime (a patched-in module only): no `DKRT_BOOT`, no runtime readiness; the case is ready 300 emulated ms after the stock firmware's ready point. */
  plain?: boolean;
  /** Plan symbols besides `DKRT_BOOT` and `DKRT_HOST_DIR`. */
  symbols?: string[];
  /** Daemon environment besides the write watch; it overrides the watch's variables. A function gets the prepared runtime plan when the daemon starts. */
  env?: Record<string, string> | ((prepared: PreparedRuntimePlan) => Record<string, string>);
  /** ColdFire-only daemon ('zero', the default via `resolveDsp`; 'cold': also without a snapshot) or with the SHARC ('sharc'). */
  dsp?: 'zero' | 'sharc' | 'cold';
  /** Configure coupled audio without loading stock firmware before the first explicit baseline. */
  startEmpty?: boolean;
  /** The first card: a function that gives its spec (default a blank store card) or `small`, a blank 1 MiB file (a test sets `DIGI_CARD_IDENTITY=small`). */
  initial?: 'small' | ((rig: { mod: (id: string, as?: string) => { name: string; file: string } }) => CardSpec);
  /** Seed every card with the stock firmware's project (Python `Env.card`). */
  project?: boolean;
  /** Uploads and project writes are part of the scenario: the card writes may be the store range and the firmware project sectors only (`common_checks_upload`). */
  uploads?: boolean;
  /** Store sectors besides the two selection sectors that the test writes on purpose (Python `store_extra`). */
  storeExtra?: number[];
  /** Use the MainPanel baseline, accepting missing filesystem verification but rejecting explicit failure; this does not stop stock verification/loading. */
  relaxFs?: boolean;
  /** Boot paused at the ready point and settle by stepping (Python `settle_ms` of 300 ms and `wait_dsp`), so a test that times events in DSP frames starts from the same frame in every run. */
  hold?: boolean;
  /** A CPU exception is part of the scenario: faults of the session are not an error. */
  faults?: boolean;
  /** Deliver genuine guest CPU exceptions during the initial baseline as well as later boots. */
  deliverExceptions?: boolean;
  /** Queue ordered panel transitions before every cold boot in this baseline, including boot-time resets. */
  initialButtons?: readonly InitialButton[];
  /** Stall threshold of the daemon in emulated ms (default 1000): for a scenario whose single guest request blocks the main loop longer. */
  stallMs?: number;
  /** More checks after the daemon has closed. */
  onClose?: (rig: CardRig, report: ReturnType<typeof runtimeSafety>) => void;
}

export interface CardRig {
  readonly device: Device;
  readonly plan: RuntimePlan;
  /** The artifact directory of the daemon. */
  readonly artifacts: string;
  /** The case of the card booted last. */
  readonly runtime: RuntimeCase;
  readonly session: Session;
  /** The image file of the card booted last. */
  readonly cardImage: string;
  /** The module store in absolute card sectors. */
  readonly store: ReturnType<typeof uploadStore>;
  /** The exported overlay of every finished case. */
  readonly overlays: readonly string[];
  /** The protocol session of the last `pausedBoot`. */
  readonly dk: DkSession;
  /** A module for a card: store path `modules/<kind>/<as>.dkm` (default `modules/<kind>/<author>-<name>.dkm`, `moduleStorePath`) and its prepared .dkm. */
  mod(id: string, as?: string): { name: string; file: string };
  /** A file below `cfg/` for a card (the fault and crash modules read their delay from it). */
  cfg(name: string, bytes: Uint8Array): { name: string; file: string };
  /** A card in a fresh directory of the run directory (plusdrive build + the records of `spec`). */
  card(spec?: CardSpec): Promise<string>;
  /** Boot the runtime on this card image in the same daemon; the previous case's evidence is collected first. */
  boot(cardImage: string, o?: { deliverExceptions?: boolean }): Promise<RuntimeCase>;
  /** `boot` of a card built from `spec`, then `settle`. */
  load(tag: string, spec: CardSpec): Promise<RuntimeCase>;
  /** The first case (the `initial` card), settled. */
  first(): Promise<RuntimeCase>;
  /** Wait for the runtime's UI and pause (the test steps time itself). */
  settle(
    runtime: TestRuntime & { epoch?: number },
    afterEpoch?: number,
    options?: { emulatedMs?: number },
  ): Promise<TestRuntime>;
  /** Poll until the UI loop was reached (and the boot is safe mode when `safe`); returns the boot's fields. */
  ready(safe?: boolean): Promise<Awaited<ReturnType<typeof bootInfo>>>;
  /** A protocol restart (the card and its guest writes stay); returns the runtime of the new boot, settled. `coreOnly`: the new boot never reaches the runtime UI, so it is not settled on it: the call returns once the new boot is installed (a new epoch), and `ready(true)` proves its boot info. */
  restart(runtime: PatchedSession, o?: { coreOnly?: boolean }): Promise<TestRuntime>;
  /** Run protocol calls on the paused daemon (the transport steps the emulator). */
  usb<T>(runtime: PatchedSession, run: (dk: DkSession) => Promise<T>): Promise<T>;
  /** The card the guest left: `base` (default the card booted last) with the overlay of its writes laid over a copy (Python `card_after`). */
  after(tag?: string, base?: string): Promise<string>;
  /** The two selection record copies as the card holds them now. */
  records(): Promise<(SelRecord | null)[]>;
  /** The sectors of the store that the guest has written, sorted. */
  storeWrites(): Promise<number[]>;
  /** All sectors the guest has written in this boot. */
  writes(tag: string): Promise<Set<number>>;
  /** Allow store sectors in the lifetime check (a test that writes them on purpose). */
  allowStoreWrites(sectors: Iterable<number>): void;
  /** Step until the guest soft-resets and the cold boot is ready (Python `Rig.wait_reset`). `steps`: the bound, in steps of 400k instructions (about 4.5 DSP blocks each). */
  waitReset(steps?: number): Promise<void>;

  // ---- paused protocol sessions (the upload tests) ----
  /** Cold boot on a card of its own that shares the image of `card`, wait for the main UI, pause and settle 300 ms; returns a protocol session that steps the emulator while it waits for a reply. */
  pausedBoot(card: string): Promise<DkSession>;
  /** The files of the store as the plusdrive reader reads them from the card after the guest's writes. */
  view(tag: string): ReturnType<typeof cardView>;
  /** `plusdrive store ls` of the card after the guest's writes. */
  ls(tag: string): ReturnType<typeof storeList>;
  /** Take a store of more than 4 GiB down to `CAP_SECTORS` on this boot (DKRT_STORE_CAP). */
  applyCap(): Promise<void>;
  /** INFO's free bytes of a card with no store: used 1, room for a one-entry table (cut by the cap). */
  freeFresh(): number;
  /** INFO's free bytes of a card whose store holds the system package only: `freeFresh` less the package's sectors and the table sector beside it. */
  freeWithPackage(): number;
  /** One request and its reply, whatever the status. `version` sends the request raw with that protocol version byte. */
  request(cmd: number, body?: Uint8Array, version?: number): Promise<{ status: number; body: Uint8Array }>;
  /** The status of one request. */
  status(cmd: number, body?: Uint8Array, version?: number): Promise<number>;
  /** One request that must be answered with status 0. */
  send(cmd: number, body?: Uint8Array): Promise<void>;
}
