// One line per USB mode switch from the events of card log files (USB_SW, modules/core/adapters/usb-bulk README). Pure: the files come from
// `readCardLog` or, offline, from `parseCardLog` on saved .bin files. Rolling and incident files overlap: an event is counted once per boot and seq.
import type { CardLogFile } from './dkupload-core.ts';
import { LOG_KIND } from './gen_proto.ts';
import { runtimeEvent } from './runtime-log.ts';

export interface SwitchSummary {
  bootSeq: number;
  switchNo: number;
  dir: string;
  port: number;
  /** Audio iso was active at REQ (null: no REQ event in the files). */
  audio: boolean | null;
  /** PHY park: ok, refused (first try failed, no later result), late (parked after a retry), never (hold ended without a park). */
  park: 'ok' | 'refused' | 'late' | 'never' | '-';
  /** ms since the request at the first RS_UP. */
  rsUpMs: number | null;
  /** Line state at the first RS_UP, from PORTSC bits 10-11. */
  lineState: 'SE0' | 'K' | 'J' | '?' | null;
  /** The clk bit (USB GPT0 counter moving) at the first RS_UP. */
  clk: number | null;
  /** The clk bit at the last event of the switch. */
  clkEnd: number | null;
  /** The ULPI wake-up result (PHY_WAKE): ok, timeout, run (RUN was set, no wake). */
  wake: 'ok' | 'timeout' | 'run' | null;
  addressedMs: number | null;
  /** Which signal showed the bus reset (connect, uri, addr0). */
  reset: string | null;
  doneMs: number | null;
  /** The last phase seen (not STATE) when there is no DONE. */
  stoppedAt: string | null;
  enumLate: boolean;
  recoveries: number;
  giveUp: boolean;
  /** RECOVERY followed by DONE: the device healed itself. */
  selfHealed: boolean;
}

const LINE_STATES = ['SE0', 'K', 'J', '?'] as const;

export interface PhyId {
  bootSeq: number;
  vendor: string;
  product: string;
  /** The ID read was refused (detail 1). */
  refused: boolean;
}

/** The PHY_ID event of each boot (logged once per boot), oldest boot first. */
export function phyIds(files: CardLogFile[]): PhyId[] {
  const out = new Map<number, PhyId>();
  for (const f of files)
    for (const raw of f.events) {
      if (raw.kind !== LOG_KIND.USB_SW || raw.seq === 0) continue;
      const fl = runtimeEvent(raw).fields;
      if (fl.phase === 'PHY_ID')
        out.set(f.bootSeq, {
          bootSeq: f.bootSeq,
          vendor: String(fl.vendor),
          product: String(fl.product),
          refused: fl.detail === 1,
        });
    }
  return [...out.values()].sort((a, b) => a.bootSeq - b.bootSeq);
}

export const phyIdLine = (p: PhyId): string =>
  `boot ${p.bootSeq}  phy vendor=${p.vendor} product=${p.product}${p.refused ? ' (read refused)' : ''}`;

/** The switches of the files, oldest boot first, in switch order. */
export function summarizeCardLog(files: CardLogFile[]): SwitchSummary[] {
  const seen = new Set<string>();
  const events: { boot: number; seq: number; e: ReturnType<typeof runtimeEvent> }[] = [];
  for (const f of files)
    for (const raw of f.events) {
      if (raw.kind !== LOG_KIND.USB_SW || raw.seq === 0) continue;
      const key = `${f.bootSeq}:${raw.seq}`;
      if (seen.has(key)) continue;
      seen.add(key);
      events.push({ boot: f.bootSeq, seq: raw.seq, e: runtimeEvent(raw) });
    }
  events.sort((a, b) => a.boot - b.boot || a.seq - b.seq);
  const out = new Map<string, SwitchSummary>();
  for (const { boot, e } of events) {
    const fl = e.fields;
    if (fl.phase === 'PHY_ID') continue;
    const key = `${boot}:${fl.switch_no}`;
    let s = out.get(key);
    if (!s) {
      s = {
        bootSeq: boot,
        switchNo: Number(fl.switch_no),
        dir: String(fl.dir),
        port: Number(fl.port),
        audio: null,
        park: '-',
        rsUpMs: null,
        lineState: null,
        clk: null,
        clkEnd: null,
        wake: null,
        addressedMs: null,
        reset: null,
        doneMs: null,
        stoppedAt: null,
        enumLate: false,
        recoveries: 0,
        giveUp: false,
        selfHealed: false,
      };
      out.set(key, s);
    }
    const phase = String(fl.phase);
    const detail = Number(fl.detail);
    if (phase !== 'STATE') s.stoppedAt = phase;
    s.clkEnd = Number(fl.clk);
    switch (phase) {
      case 'PHY_WAKE':
        s.wake = (['ok', 'timeout', 'run'] as const)[detail] ?? s.wake;
        break;
      case 'REQ':
        s.audio = fl.audio === 1;
        break;
      case 'PHY_PARK':
        s.park = (['ok', 'refused', 'late'] as const)[detail] ?? s.park;
        break;
      case 'PHY_BUSY':
        if (detail === 3) s.park = 'never';
        else if (s.park === '-' || s.park === 'ok') s.park = 'refused';
        break;
      case 'RS_UP':
        if (s.rsUpMs === null) {
          s.rsUpMs = Number(fl.ms);
          s.clk = Number(fl.clk);
          s.lineState = LINE_STATES[(parseInt(String(fl.portsc), 16) >>> 10) & 3]!;
        }
        break;
      case 'ADDRESSED':
        s.addressedMs = Number(fl.ms);
        s.reset = String(fl.reset);
        break;
      case 'ENUM_LATE':
        s.enumLate = true;
        break;
      case 'RECOVERY':
        s.recoveries++;
        break;
      case 'GIVEUP':
        s.giveUp = true;
        break;
      case 'DONE':
        s.doneMs = Number(fl.ms);
        break;
    }
  }
  const list = [...out.values()];
  for (const s of list) {
    s.selfHealed = s.recoveries > 0 && s.doneMs !== null;
    if (s.doneMs !== null) s.stoppedAt = null;
  }
  return list;
}

const sec = (ms: number | null) => (ms === null ? '-' : `${(ms / 1000).toFixed(3)}s`);

/** One line for a switch of `summarizeCardLog`. */
export function switchLine(s: SwitchSummary): string {
  const marks = [
    s.enumLate ? 'ENUM_LATE' : '',
    s.recoveries ? `RECOVERY x${s.recoveries}` : '',
    s.giveUp ? 'GIVEUP' : '',
    s.selfHealed ? 'self-healed' : '',
  ].filter(Boolean);
  return [
    `boot ${s.bootSeq}`,
    `sw ${String(s.switchNo).padStart(3)}`,
    s.dir.padEnd(9),
    `port ${s.port}`,
    `park=${s.park}`,
    `rs_up=${sec(s.rsUpMs)}`,
    `line=${s.lineState ?? '-'}`,
    `clk=${s.clk ?? '-'}`,
    `addressed=${sec(s.addressedMs)}${s.reset ? `(${s.reset})` : ''}`,
    `done=${s.doneMs === null ? `no (${s.stoppedAt ?? '-'})` : sec(s.doneMs)}`,
    `audio=${s.audio === null ? '-' : s.audio ? 1 : 0}`,
    ...(s.doneMs === null ? [`clk_end=${s.clkEnd ?? '-'}`] : []),
    ...(s.wake ? [`wake=${s.wake}`] : []),
    ...marks,
  ].join('  ');
}
