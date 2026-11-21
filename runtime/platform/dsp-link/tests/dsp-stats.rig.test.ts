/**
 * The DSP stats window in the block tail of core/dsp-link (docs/plans/profiling.md 13.1, P8d) on the SHARC-only rig, DT2 and DN2: window 3 (the control
 * words), the window pointer the services test, the fold of a block (blocks, the largest stock block delta, overruns, the largest block of each row) and the
 * pages of the window streamed through the acknowledgement slot.
 *
 * The rig calls `_dl_hook` the way the stock block handler reaches it: I4 the reply record, R2 the record's first word (the stock block delta rotated by 16
 * bits), I6 the handler's frame whose word -3 points to the receive ring. The ring holds the chunk the ColdFire would have written into the frame tail.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  DATA_BASE,
  CODE_BASE,
  ROOT,
  SW_ALIAS_BASE,
  STOCK_BP,
  assembleSource,
  link,
  nw,
  openRig,
  placeData,
  sharcIncludeDirs,
  type CallResult,
  type Op,
  type RigPort,
} from '@digi/sharc-rig-kit';

const MOD = path.join(ROOT, 'modules/core/adapters/dsp-link');
const [REC, RING, FRAME, DVDIAG, FXDIAG, DVREC, FXREC] = [
  0x20090000, 0x20092000, 0x20093800, 0x20094000, 0x20094100, 0x20094200, 0x20094210,
];
const DL_MAGIC = 0xd15c;
const ACK_OFF = 0x60;
const STATS_MAGIC = 0x57a7;
const WINDOW_WORDS = 100;
const HEAD = 4;
const ROW = 6;
const u32 = (n: number): number => n >>> 0;
const rotl16 = (d: number): number => u32((d << 16) | (d >>> 16));

/** The tail as the rig runs it. `withoutStats` drops the whole stats part of the tail (the test of the window pointer and the fold): the baseline of the off cost. */
function build(device: 'dt2' | 'dn2', withoutStats: boolean) {
  const file = path.join(MOD, 'src/dsp', `${device}.s`);
  const object = assembleSource(file, sharcIncludeDirs(MOD), (t) => {
    // the area is 512 KB: the rig needs a few words of it only (window 0 is not used here)
    let x = t.replace(/\.PKBSS _dl_area, DL_AREA_BYTES;/, '.PKBSS _dl_area, 4096;');
    if (withoutStats) x = x.replace(/^_dl_out:\n[\s\S]*?^_dl_nost:\n/m, '_dl_out:\n');
    return x;
  });
  const tailOff = Number(/#define DL_TAIL_OFF (\d+)/.exec(readFileSync(path.join(MOD, `build/gen/device-${device}.inc`), 'utf8'))![1]);
  const layout = placeData([object], DATA_BASE);
  const externs: Record<string, number> = {
    _dl_back: STOCK_BP,
    _dl_vrows: 0,
    _dl_vcount: 0,
    _dl_frows: 0,
    _dl_fcount: 0,
    _dv_diag: DVDIAG,
    _dv_rec: DVREC,
    _fx_diag: FXDIAG,
    _fx_rec: FXREC,
    _sp_tail: 0,
    _dk_rec: 0,
    ...layout.data,
  };
  const linked = link(object, CODE_BASE, externs);
  return { linked, data: layout.data, inits: layout.inits, hook: linked.symbols._dl_hook!, tailOff };
}

type Built = ReturnType<typeof build>;

class TailRig {
  private port!: RigPort;
  private readonly pending: Op[] = [];
  private constructor(readonly b: Built) {}

  static async open(b: Built): Promise<TailRig> {
    const rig = new TailRig(b);
    const poke: [number, number][] = [];
    for (const init of b.inits) {
      const view = new DataView(init.bytes.buffer, init.bytes.byteOffset, init.bytes.byteLength);
      for (let k = 0; k < init.bytes.length / 4; k++)
        poke.push([nw(init.address) + k, init.bss ? 0xffffffff : view.getUint32(4 * k, true)]);
    }
    for (const [base, words] of [
      [REC, 0x800],
      [RING, 0x400],
      [FRAME, 8],
      [DVDIAG, 32],
      [FXDIAG, 8],
      [DVREC, 4],
      [FXREC, 4],
    ] as const)
      for (let k = 0; k < words; k++) poke.push([nw(base) + k, 0]);
    const { port } = await openRig({
      return_sw: 0xb9f100,
      breaks: [STOCK_BP],
      code: [{ address: SW_ALIAS_BASE + 2 * b.linked.baseSw, hex: b.linked.codeBoot }],
      ops: [{ poke }],
    });
    rig.port = port;
    return rig;
  }

  close = (): Promise<void> => this.port.close();
  poke(address: number, word: number): void {
    const last = this.pending.at(-1);
    if (last && 'poke' in last) last.poke.push([address, u32(word)]);
    else this.pending.push({ poke: [[address, u32(word)]] });
  }
  sym = (name: string): number => this.b.data[name]!;

  /** A chunk of the ColdFire in the ring: `n` payload words into window `win` at word `off`, transfer `xid`; `end` = an END chunk. */
  chunk(o: { win: number; off: number; payload: number[]; xid: number; idx?: number; end?: boolean }): void {
    const idx = o.idx ?? 0;
    const n = o.end ? 0 : o.payload.length;
    const w0 = u32(DL_MAGIC | (idx << 16) | (o.win << 30));
    const w1 = u32((o.end ? 1 << 26 : o.off | (n << 17)) | (o.xid << 27));
    let s1 = u32(w0 + w1);
    let s2 = u32(w0 + s1);
    const words = [w0, w1, ...(o.end ? [] : o.payload)];
    for (const w of o.end ? [] : o.payload) {
      s1 = u32(s1 + w);
      s2 = u32(s2 + s1);
    }
    words.push(u32(s1 ^ s2));
    words.forEach((w, k) => this.poke(nw(RING + this.b.tailOff) + k, w));
  }

  /** One call of the tail. The record is cleared first, as the engine does. */
  async block(stockDelta: number, reads: [number, number][] = []): Promise<{ res: CallResult; words: (number | null)[][] }> {
    for (let k = 0; k < 0x180; k++) this.poke(nw(REC) + k, 0);
    this.poke(nw(FRAME) - 3, RING);
    const regs = { I4: REC, I6: FRAME, I7: FRAME, R2: rotl16(stockDelta), R1: 0, M5: 0, M6: 1, L2: 0, L3: 0 };
    const [res] = (await this.port.run([
      ...this.pending.splice(0),
      { call: { pc: this.b.hook, regs, max_steps: 5000, reads, read_regs: [] } },
    ])) as [CallResult];
    const near = Object.entries(this.b.linked.symbols)
      .filter(([, at]) => at <= res.final_pc)
      .sort((x, y) => y[1] - x[1])[0];
    expect(res.halt, `the tail returns to the block handler (stopped at ${near?.[0]} + ${res.final_pc - (near?.[1] ?? 0)})`).toBe('breakpoint');
    expect(res.final_pc).toBe(STOCK_BP);
    return { res, words: res.reads };
  }

  async peek(address: number, count = 1): Promise<number[]> {
    const { words } = await this.block(0, [[nw(address), count]]).catch(() => ({ words: [[]] }));
    return (words[0] ?? []).map((x) => x ?? 0);
  }
}

const row = (w: readonly number[], r: number) => w.slice(HEAD + ROW * r, HEAD + ROW * (r + 1));

for (const device of ['dt2', 'dn2'] as const) {
  describe(`stats in the block tail on ${device.toUpperCase()}`, () => {
    const on = build(device, false);
    const base = build(device, true);

    /** Window 3 written (stats on, generation, period) and the transfer ended, as the runtime's `dk_dsp_stats_ctl` would. */
    async function started(b: Built, o: { on?: number; gen?: number; period?: number } = {}): Promise<TailRig> {
      const rig = await TailRig.open(b);
      rig.chunk({ win: 3, off: 0, payload: [o.on ?? 1, o.gen ?? 7, o.period ?? 1000], xid: 1 });
      await rig.block(0);
      rig.chunk({ win: 3, off: 0, payload: [], xid: 1, end: true });
      await rig.block(0);
      // the two blocks above were folded too: start the counts from 0 (what a clear generation does)
      for (let k = 0; k < 104; k++) rig.poke(nw(rig.sym('_dk_stats')) + k, 0);
      rig.poke(nw(rig.sym('_dl_stpage')), 0);
      return rig;
    }

    it('window 3 sets the window pointer and the words the services test, clears the window and stops it again', async () => {
      const rig = await TailRig.open(on);
      try {
        const win = rig.sym('_dk_stats');
        rig.chunk({ win: 3, off: 0, payload: [1, 7, 1000], xid: 1 });
        const { words } = await rig.block(0, [
          [nw(win), 104],
          [nw(DVDIAG) + 31, 1],
          [nw(FXDIAG) + 7, 1],
          [nw(rig.sym('_dl_stptr')), 1],
          [nw(rig.sym('_dl_stgen')), 1],
        ]);
        expect(words[0]![0], 'this block is the first one folded').toBe(1);
        expect(words[0]!.slice(1).every((x) => x === 0), 'the poisoned window and its private words are cleared').toBe(true);
        expect([words[1]![0], words[2]![0], words[3]![0]], 'the pointers').toEqual([win, win, win]);
        expect(words[4]![0]).toBe(7);
        rig.chunk({ win: 3, off: 0, payload: [0, 7, 1000], xid: 1, idx: 1 });
        const off = await rig.block(0, [
          [nw(DVDIAG) + 31, 1],
          [nw(FXDIAG) + 7, 1],
          [nw(rig.sym('_dl_stptr')), 1],
        ]);
        expect(off.words.map((w) => w[0]), 'stats off: the pointers are 0').toEqual([0, 0, 0]);
      } finally {
        await rig.close();
      }
    });

    it('a block is folded: blocks, the largest stock delta, overruns and the largest block of each row', async () => {
      const rig = await started(on);
      try {
        const win = rig.sym('_dk_stats');
        // the services' rows as spans leave them in a block: row 3 (calls 2, cycles 500, largest call 300, largest block so far 200, block sum 500)
        const r3 = win + 4 * (HEAD + ROW * 3);
        [2, 500, 0, 300, 200, 500].forEach((v, k) => rig.poke(nw(r3) + k, v));
        const a = await rig.block(1200, [[nw(win), 104]]);
        const w1 = a.words[0]!.map((x) => x ?? 0);
        expect(w1.slice(0, 3), 'blocks, largest stock delta, overruns (1200 >= period 1000)').toEqual([1, 1200, 1]);
        expect(row(w1, 3), 'the largest block takes the sum, the sum restarts').toEqual([2, 500, 0, 300, 500, 0]);
        const b = await rig.block(900, [[nw(win), 104]]);
        expect(b.words[0]!.slice(0, 3), 'a block under the period').toEqual([2, 1200, 1]);
      } finally {
        await rig.close();
      }
    });

    it('a period of 0 counts no overrun', async () => {
      const rig = await started(on, { period: 0 });
      try {
        const a = await rig.block(5000, [[nw(rig.sym('_dk_stats')), 4]]);
        expect(a.words[0]!.slice(0, 3)).toEqual([1, 5000, 0]);
      } finally {
        await rig.close();
      }
    });

    it('one page of the window goes into the acknowledgement slot per block, pages 0 to 49 in turn, with the generation', async () => {
      const rig = await started(on);
      try {
        const win = rig.sym('_dk_stats');
        for (let r = 0; r < 16; r++) rig.poke(nw(win) + HEAD + ROW * r + 3, 0x2000 + r); // the largest call of each row
        const slot = nw(REC + ACK_OFF);
        for (let p = 0; p < 52; p++) {
          const { words } = await rig.block(0, [[nw(win), WINDOW_WORDS], [slot, 3]]);
          const [w, page] = [words[0]!.map((x) => x ?? 0), words[1]!.map((x) => x ?? 0)];
          const q = p % 50;
          expect(page[0], `page ${p}: tag`).toBe(u32(STATS_MAGIC | (q << 16) | (7 << 24)));
          expect([page[1], page[2]], `page ${p}: the window words after the fold`).toEqual([w[2 * q], w[2 * q + 1]]);
        }
      } finally {
        await rig.close();
      }
    });

    it('a block that acknowledges a transfer keeps the slot: no page, the page counter moves on', async () => {
      const rig = await started(on);
      try {
        rig.chunk({ win: 3, off: 0, payload: [1, 7, 1000], xid: 2 });
        const a = await rig.block(0, [[nw(REC + ACK_OFF), 3]]);
        expect(a.words[0]![0]! & 0xffff, 'the acknowledgement, not a page').toBe(0xa5d1);
      } finally {
        await rig.close();
      }
    });

    it('a new clear generation zeroes the window and restarts the pages', async () => {
      const rig = await started(on);
      try {
        const win = rig.sym('_dk_stats');
        await rig.block(1500);
        await rig.block(1500);
        rig.chunk({ win: 3, off: 0, payload: [1, 8, 1000], xid: 3 });
        const { words } = await rig.block(0, [[nw(win), 104]]);
        expect(words[0]!.slice(0, 3), 'the clear zeroes the window, this block is the first one folded').toEqual([1, 0, 0]);
        rig.chunk({ win: 3, off: 0, payload: [], xid: 3, end: true });
        const next = await rig.block(0, [[nw(win), 4], [nw(REC + ACK_OFF), 3]]);
        expect(next.words[0]!.slice(0, 3), 'counts again from 1, not on from the old counts').toEqual([2, 0, 0]);
        expect(next.words[1]![0], 'page 1 of generation 8 (page 0 was the block of the clear, which acknowledges its transfer)').toBe(
          u32(STATS_MAGIC | (1 << 16) | (8 << 24)),
        );
      } finally {
        await rig.close();
      }
    });

    it('stats off: the tail pays 3 instructions per block and writes neither the window nor the slot', async () => {
      const rig = await started(on, { on: 0 });
      const ref = await started(base, { on: 0 });
      try {
        const a = await rig.block(1200, [[nw(rig.sym('_dk_stats')), 4], [nw(REC + ACK_OFF), 3]]);
        const b = await ref.block(1200);
        expect(a.res.instructions - b.res.instructions).toBe(3);
        expect(a.words[1]!.every((x) => (x ?? 0) === 0), 'no page').toBe(true);
      } finally {
        await rig.close();
        await ref.close();
      }
    });

    it('stats on: what the fold and the page add to a block of the tail', async () => {
      const rig = await started(on);
      const off = await started(on, { on: 0 });
      try {
        const a = await rig.block(1200);
        const b = await off.block(1200);
        const added = a.res.instructions - b.res.instructions;
        console.log(`${device}: stats on adds ${added} instructions per block to the tail`);
        expect(added).toBeLessThan(250);
      } finally {
        await rig.close();
        await off.close();
      }
    });
  });
}
