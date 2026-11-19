// The fit rule: does a module, from the needs inferred from its contents, fit the capacity of a device (docs/plans/what-fits.md)?
// The rule exists once, in Rust (modules/core/runtime/fit: the code the DIGIKIT menu runs). This file only calls it through `pk_fit` of
// patchkit.wasm (dkneeds.ts) and gives the types. The reader is set once, where patchkit.wasm is loaded (`setFitReader`), so the callers stay
// synchronous. Written without TS-only runtime syntax so node can import it.

/** CAPACITY as the session returns it: kind name -> limit, used, largest free piece. */
export type Caps = Record<string, { limit: number; used: number; largest: number }>;

/** The needs of a .dkm as `dkneeds` infers them: `file_size` and these fields (`Needs::fields` in packages/dkneeds). Missing = 0. */
export type Needs = { file_size?: number } & Partial<
  Record<
    | 'ARENA'
    | 'ARENA_XFER'
    | 'DSP_AREA'
    | 'DSP_ALIGN'
    | 'MACHINE_ROWS'
    | 'VOICE_ROWS'
    | 'PARAM_ROWS'
    | 'PAGE_ROWS'
    | 'FX_SLOTS'
    | 'DSP_COST_VOICE'
    | 'DSP_COST_FX',
    number
  >
>;

/**
 * The first short resource: its CAPACITY kind name, what the module needs and what is free (bytes, or a count), and its texts. `text` and `brief`
 * are the ones the device shows (`DSP 400K > 254K FREE`, `DSP 400K>254K`); `describe` is `DSP memory 400 KiB needed, 254 KiB free` and
 * `describe_by` is `DSP memory 146 KiB short`.
 */
export type Short = {
  resource: string;
  need: number;
  free: number;
  text: string;
  brief: string;
  describe: string;
  describe_by: string;
};

/** A request to the rule. With `empty` the capacity first becomes the one of a runtime that has loaded nothing (the next boot). */
export type FitRequest = { caps: Caps; needs: Needs[]; empty?: boolean };

/**
 * The answer: `each` is per need (null: it fits the capacity alone), `set` is null when the needs fit in turn (the sum rule: it does not model
 * fragmentation, so it is conservative), else the index of the first that does not and what it is short of. `line` is the load line
 * (`DSP 52% CPU 1%`) of the capacity once the needs that fit are taken.
 */
export type FitAnswer = { each: (Short | null)[]; set: (Short & { index: number }) | null; line: string };

/** A key as the key check names it: the record kind and the key word. */
export type KeyRef = { kind: number; key: number };

/** The key rules of a file (`pk_keys_check`): a key it provides twice, and, against another file, a key both provide (`shared`) and a key only the other provides (`missing`). */
export type KeysCheck = { repeat: KeyRef | null; shared: KeyRef | null; missing: KeyRef | null };

export type FitReader = { fit(req: FitRequest): FitAnswer; keys?(bytes: Uint8Array, other?: Uint8Array): KeysCheck };

let reader: FitReader | undefined;

/** Set the reader: the `DkNeeds` that holds patchkit.wasm (dkneeds.ts). */
export function setFitReader(r: FitReader): void {
  reader = r;
}

/** Ask the rule. */
export function fit(req: FitRequest): FitAnswer {
  if (!reader) throw new Error('the fit rule is not loaded (patchkit.wasm)');
  return reader.fit(req);
}

/** null when the module fits `caps`, else the first short resource. */
export function fitsNow(n: Needs, caps: Caps, empty = false): Short | null {
  return fit({ caps, needs: [n], empty }).each[0] ?? null;
}

/** null when all the modules fit in turn into the empty runtime of the next boot (`empty`) or into `caps`, else the first that does not. */
export function fitsSet(needs: Needs[], caps: Caps, empty = false): (Short & { index: number }) | null {
  return fit({ caps, needs, empty }).set;
}

/** The key rules of the module file `bytes` against `other` (see `KeysCheck`); throws when the reader is not loaded or a file is not a module. */
export function keysCheck(bytes: Uint8Array, other?: Uint8Array): KeysCheck {
  if (!reader?.keys) throw new Error('the key check is not loaded (patchkit.wasm)');
  return reader.keys(bytes, other);
}

const KIND_NAMES: Record<number, string> = { 2: 'machine', 4: 'effect', 8: 'LFO shape' };

/** A key for a person: `machine key 0x00a10100`. */
export function keyText(k: KeyRef): string {
  return `${KIND_NAMES[k.kind] ?? `kind ${k.kind}`} key 0x${k.key.toString(16).padStart(8, '0')}`;
}

/**
 * The warnings of the key rules for the new file `name` that is about to go on the card: it provides a key twice, or a key of a module that is on the card already
 * (`others`: the other files' name and bytes; the planner refuses the second module of a key at the next boot), or it lacks a key of the version on the card (`old`: a
 * reload refuses it, and kits and projects name those keys). The device refuses these too; this tells the user before the upload. A file the check cannot read gives none.
 */
export function keyWarnings(
  name: string,
  bytes: Uint8Array,
  others: { name: string; bytes: Uint8Array }[],
  old?: Uint8Array,
): string[] {
  const out: string[] = [];
  try {
    const own = keysCheck(bytes, old);
    if (own.repeat) out.push(`${name} has the ${keyText(own.repeat)} twice`);
    if (own.missing)
      out.push(`${name} lacks the ${keyText(own.missing)} of the version on the card: a reload is refused`);
    for (const o of others) {
      const r = keysCheck(bytes, o.bytes);
      if (r.shared)
        out.push(`${name} and ${o.name} both have the ${keyText(r.shared)}: the second is refused at the next boot`);
    }
  } catch {
    // not a module this reader understands: the device says so
  }
  return out;
}
