import { BOOT_WAIT_MS, EMULATED_POLL_MS } from '@digi/dk-session';
import { emulatorClock, pollUntil, waitClock, type Session } from '@digi/test-support';
import { SEL_MAX } from './card-record.ts';
import { rpc, word } from './guest-access.ts';
import { stateName } from './load-states.ts';
import { planSymbol, type RuntimePlan } from './runtime-plan.ts';

/** The guest's boot, card and crash words, and the test mailbox (commands, pokes). Port of the guest helpers of pk_dkrt_boot.py. */

export const E_RANGE = -46;
export const E_CARD_RO = -47;
export const CMD = { LOAD: 1, SELECT: 3, RESTART: 4, SELECT_ONE: 5, CARD_PROBE: 7 };
const MB_CMD = 0;
const MB_SEQ = 4;
const MB_RESULT = 8;
const MB_ARG = 12;

// ---- guest memory and the test mailbox ----------------------------------------------------------------------------------------------

const BI_FIELDS = [
  'magic',
  'version',
  'safe',
  'reason',
  'counter',
  'seq',
  'copy',
  'count',
  'loaded',
  'uiReached',
] as const;
/** `DkrtBootInfo.magic`: 'DKBT'. */
const BI_MAGIC = 0x444b4254;
const SAFE_NAMES = ['none', 'counter', 'key', 'flag', 'dsp', 'panic', 'read'];
const CARD_FIELDS = [
  'state',
  'firstNonblank',
  'hash',
  'refused',
  'refusedRange',
  'readErrors',
  'checked',
  'claims',
  'writes',
] as const;
const CARD_NAMES = ['unknown', 'ours', 'blank', 'foreign', 'unreadable', 'too small'];
const CRASH_FIELDS = [
  'found',
  'pc',
  'vec',
  'module',
  'stable',
  'ramKept',
  'installed',
  'bad',
  'hang',
  'saved',
  'panic',
  'tail',
] as const;
/** `LOADING` in a module's boot result: its DSP part has not followed yet. */
export const E_LOADING = 1;
/** Store statuses of the upload protocol (the numbers of gen_proto's STATUS table). */
export const E_NOSTORE = -61;
export const E_INUSE = -62;
export const E_PEEK = -63;

async function words<const F extends readonly string[]>(
  session: Session,
  plan: RuntimePlan,
  symbol: string,
  fields: F,
) {
  const base = planSymbol(plan, symbol);
  const values = await Promise.all(fields.map((_, i) => word(session, base + 4 * i)));
  return Object.fromEntries(fields.map((k, i) => [k, values[i]!])) as Record<F[number], number>;
}

/** `DKRT_BOOT`: the boot's fields, per-module results (signed, `results` has `count` of them), `coreCause` (CORE_*, 0 when the boot is not core-only; the word that was `dsp_wait`) and `stable`. */
export async function bootInfo(session: Session, plan: RuntimePlan) {
  const d = await words(session, plan, 'DKRT_BOOT', BI_FIELDS);
  const base = planSymbol(plan, 'DKRT_BOOT') + 4 * BI_FIELDS.length;
  const tail = await Promise.all(Array.from({ length: SEL_MAX + 2 }, (_, i) => word(session, base + 4 * i)));
  const allResults = tail.slice(0, SEL_MAX).map((v) => v | 0);
  // after `needs` (SEL_MAX * NEEDS_LEN bytes at offset 80 of DkrtBootInfo, runtime/core/abi.toml): `states[SEL_MAX]` (bytes), `detail[SEL_MAX][2]`
  const statesAt = planSymbol(plan, 'DKRT_BOOT') + BI_STATES_AT;
  const stateWords = await Promise.all([0, 1].map((i) => word(session, statesAt + 4 * i)));
  const states = Array.from({ length: SEL_MAX }, (_, i) => (stateWords[i >> 2]! >>> (24 - 8 * (i & 3))) & 255);
  const detailWords = await Promise.all(
    Array.from({ length: SEL_MAX * 2 }, (_, i) => word(session, statesAt + 8 + 4 * i)),
  );
  const detail = Array.from(
    { length: SEL_MAX },
    (_, i) => [detailWords[2 * i]!, detailWords[2 * i + 1]!] as [number, number],
  );
  return {
    ...d,
    results: allResults.slice(0, d.count),
    allResults,
    /** The state of each selected module (`[load.state]`: `STATE_NAME[n]`). */
    states: states.slice(0, d.count),
    /** What the plan refused each module on (`DkrtBootInfo.detail`). */
    detail: detail.slice(0, d.count),
    coreCause: tail[SEL_MAX]!,
    stable: tail[SEL_MAX + 1]!,
    reasonName: SAFE_NAMES[d.reason] ?? '?',
  };
}

/** The offset of `DkrtBootInfo.states` (after the 10 words before `results`, the results, core_cause, stable and `needs`; asserted by `abi.toml`'s layout check). */
const BI_STATES_AT = 176;

/** The states of a module that has not finished its load: not planned, planned, `init` running, or the DSP part still going (`[load.state]`). */
const UNSETTLED_STATES = new Set(['SELECTED', 'PLANNED', 'INIT', 'DSP_LOADING']);

/**
 * Whether a boot info is final: no state is unsettled and `loaded` equals the number of ACTIVE modules. The runtime sets a module's state and raises `loaded` in two
 * steps (`report_module`, `report_dsp`), and the session pauses between instructions: a read in between sees the new state with the old count.
 */
export function bootInfoFinal(info: { loaded: number; states: number[] }): boolean {
  const names = info.states.map((n) => stateName(n));
  return !names.some((n) => UNSETTLED_STATES.has(n)) && info.loaded === names.filter((n) => n === 'ACTIVE').length;
}

/**
 * `bootInfo` once this boot has written its info and every selected module has a final state (`DkrtBootInfo.states`: ACTIVE, REFUSED, FAILED, SKIPPED or UNLOADED):
 * the boot has finished loading its selection. The result words do not tell this: 0 is also the unwritten initial value, so a module not yet started reads 0 like a
 * loaded one. Written means `magic` is valid (the boot sets it first) and `count` is set (the boot copies it from the selection record before any load, and `states`
 * is cut to it, so a zero `count` before that would pass for an empty list); a boot with an empty selection has no later word, so there `uiReached` stands in.
 * `expected` is the number of modules the caller selected, when it knows it. The DSP part of a module follows its ColdFire part over the first UI-loop passes. Waits on that
 * state (`BOOT_WAIT_MS` only guards a hang); the session must be paused. Throws, naming the states seen, on the guard.
 */
export async function bootInfoLoaded(session: Session, plan: RuntimePlan, expected?: number) {
  const settled = (info: Awaited<ReturnType<typeof bootInfo>>) => {
    const written = info.magic === BI_MAGIC && info.version !== 0 && (info.count > 0 || info.uiReached !== 0);
    const counted = expected === undefined || info.count >= expected;
    return written && counted && bootInfoFinal(info);
  };
  let last: Awaited<ReturnType<typeof bootInfo>> | undefined;
  return pollUntil(emulatorClock(session), async () => (last = await bootInfo(session, plan)), settled, {
    withinMs: BOOT_WAIT_MS,
    intervalMs: EMULATED_POLL_MS,
    message: () => {
      const seen = last?.states.map((n) => stateName(n)).join(',') || 'none';
      return (
        `boot info not settled: magic 0x${((last?.magic ?? 0) >>> 0).toString(16)}, count ${last?.count}` +
        `${expected === undefined ? '' : ` (expected ${expected})`}, uiReached ${last?.uiReached}, states [${seen}]`
      );
    },
  });
}

/** `DKRT_CARD`: state, the first non-blank sector, its crc32, writes refused and done. */
export async function cardInfo(session: Session, plan: RuntimePlan) {
  const d = await words(session, plan, 'DKRT_CARD', CARD_FIELDS);
  return { ...d, stateName: CARD_NAMES[d.state] ?? '?' };
}

/** `DKRT_CRASH`: what the boot found of a crash record. */
export const crashInfo = (session: Session, plan: RuntimePlan) => words(session, plan, 'DKRT_CRASH', CRASH_FIELDS);

export const pokeWord = (session: Session, addr: number, value: number) =>
  rpc(session, 'mem.cf.poke', { addr, value: value >>> 0 });

/** Set a firmware or runtime variable by its plan symbol (Python `M.sym` + `_poke_u32`). */
export const pokeKnob = (session: Session, plan: RuntimePlan, name: string, value: number) =>
  pokeWord(session, planSymbol(plan, name), value);

/** A mailbox command with binary arguments (big-endian words, zero padded); returns its signed result. */
export async function postRaw(session: Session, plan: RuntimePlan, cmd: number, data: Buffer): Promise<number> {
  const mb = planSymbol(plan, 'DKRT_MAILBOX');
  const arg = Buffer.concat([data, Buffer.alloc(-data.length & 3)]);
  for (let i = 0; i < arg.length; i += 4) await pokeWord(session, mb + MB_ARG + i, arg.readUInt32BE(i));
  const seq0 = await word(session, mb + MB_SEQ);
  await pokeWord(session, mb + MB_CMD, cmd);
  for (let i = 0; i < 200; i++) {
    await waitClock(emulatorClock(session), 20);
    if ((await word(session, mb + MB_CMD)) === 0 && (await word(session, mb + MB_SEQ)) !== seq0) break;
  }
  return (await word(session, mb + MB_RESULT)) | 0;
}

/** Ask the runtime's card-write function to write `nbytes` at `sector`: 0, `E_RANGE` or `E_CARD_RO`. Only for sectors outside the store or on a card that is not ours. */
export const probeWrite = (session: Session, plan: RuntimePlan, sector: number, nbytes: number) => {
  const b = Buffer.alloc(8);
  b.writeUInt32BE(sector, 0);
  b.writeUInt32BE(nbytes, 4);
  return postRaw(session, plan, CMD.CARD_PROBE, b);
};

/** The selection through the mailbox (`dkrt_selection_set`); returns its result. */
export const setSelection = (session: Session, plan: RuntimePlan, paths: string[]) =>
  postRaw(session, plan, CMD.SELECT, Buffer.from(paths.join('\0') + '\0\0'));
