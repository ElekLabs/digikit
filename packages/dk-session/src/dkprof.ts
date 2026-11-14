// Profiles, address lookup and module DSP analysis (packages/dkprof, `dkprof.wasm`): the code `wb` and the workbench call. Every call is JSON in and
// JSON out through linear memory (docs in packages/dkprof/src/wasm.rs); a request with `format: 'text'` gets `{ text }`, the view of the answer.
// Written without TS-only runtime syntax so node can import it.

export class ProfError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ProfError';
  }
}

interface Exports {
  memory: WebAssembly.Memory;
  dp_alloc(n: number): number;
  dp_free(p: number, n: number): void;
  dp_addr_lookup(req: number, len: number): number;
  dp_load_table(req: number, len: number): number;
  dp_profile_report(req: number, len: number): number;
  dp_module_analyse(dkm: number, dkmLen: number, dko: number, dkoLen: number, text: number): number;
  dp_out_ptr(): number;
  dp_out_len(): number;
}

export type Cpu = 'cf' | 'sharc';

/** An address map (docs/ADDRMAP.md) and a load table are passed through as the JSON the tools write. */
export type AddrMapJson = Record<string, unknown>;
export interface LoadTableJson {
  modules: { id: string; base: number; size?: number; dsp_base?: number; fini?: number }[];
}

export interface LookupRequest {
  maps: AddrMapJson[];
  loads?: LoadTableJson;
  /** Raw bytes of the runtime symbols as hex (`DKRT_STATE`, `DKRT_DSP`, `dyndsp_info`). */
  snapshot?: Record<string, string>;
  /** Module id to the software PC where its DSP image starts. */
  dsp_base?: Record<string, number>;
  ret?: boolean;
  addrs: { cpu: Cpu; addr: number }[];
}
export interface LookupResult {
  cpu: Cpu;
  addr: number;
  name: string | null;
  text: string | null;
  layer: 'our' | 'anchor' | 'fw' | null;
  sym_addr: number | null;
  offset: number | null;
  size: number | null;
  module: string | null;
}
export interface LookupAnswer {
  unloaded: string[];
  results: LookupResult[];
}

export interface ProfileReportRequest {
  /** A `dk.profile/1` document. */
  profile: unknown;
  maps?: AddrMapJson[];
  loads?: LoadTableJson;
  top?: number;
}

export class DkProf {
  private x: Exports;
  private constructor(x: Exports) {
    this.x = x;
  }
  static async create(wasm: BufferSource): Promise<DkProf> {
    const { instance } = await WebAssembly.instantiate(wasm, {});
    return new DkProf(instance.exports as unknown as Exports);
  }

  private withBytes<T>(b: Uint8Array, f: (p: number) => T): T {
    const p = this.x.dp_alloc(b.length);
    new Uint8Array(this.x.memory.buffer, p, b.length).set(b);
    try {
      return f(p);
    } finally {
      this.x.dp_free(p, b.length);
    }
  }

  private answer(rc: number): unknown {
    const n = this.x.dp_out_len();
    const text = new TextDecoder().decode(new Uint8Array(this.x.memory.buffer, this.x.dp_out_ptr(), n));
    const out = JSON.parse(text);
    if (rc !== 0) throw new ProfError(out.error ?? 'dkprof failed');
    return out;
  }

  private json(call: (p: number, n: number) => number, req: object): unknown {
    const bytes = new TextEncoder().encode(JSON.stringify(req));
    return this.answer(this.withBytes(bytes, (p) => call(p, bytes.length)));
  }

  /** Names for addresses. With `format: 'text'` the answer is `{ text }`. */
  addrLookup(req: LookupRequest & { format?: 'text' }): LookupAnswer & { text?: string } {
    return this.json((p, n) => this.x.dp_addr_lookup(p, n), req) as LookupAnswer;
  }

  /** The load table of a snapshot of the runtime's tables; without `snapshot` the symbols and byte counts to read (`{ reads }`). */
  loadTable(req: {
    maps: AddrMapJson[];
    snapshot?: Record<string, string>;
  }): LoadTableJson | { reads: { symbol: string; len: number }[] } {
    return this.json((p, n) => this.x.dp_load_table(p, n), req) as LoadTableJson;
  }

  /** The named self aggregates of a `dk.profile/1` document (a `Report`, or `{ text }` with `format: 'text'`). */
  profileReport(req: ProfileReportRequest & { format?: 'text' }): Record<string, unknown> {
    return this.json((p, n) => this.x.dp_profile_report(p, n), req) as Record<string, unknown>;
  }

  /** The static analysis of the DSP section of a `.dkm` with the symbols of its DSP `.dko.json` (`dksharc::Analysis`, or `{ text }`). */
  moduleAnalyse(dkm: Uint8Array, dko: Uint8Array, text = false): Record<string, unknown> {
    const rc = this.withBytes(dkm, (a) =>
      this.withBytes(dko, (b) => this.x.dp_module_analyse(a, dkm.length, b, dko.length, text ? 1 : 0)),
    );
    return this.answer(rc) as Record<string, unknown>;
  }
}
