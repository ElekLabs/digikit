// Calibration profiles (`dk.calprofile/1`): what to play and how long to capture. Pure; the runner plays them.
import { parse as parseToml } from 'smol-toml';
import type { Args, StepRef } from './steps.ts';

export const CALPROFILE_SCHEMA = 'dk.calprofile/1';
/** The case the fit takes the stock load from: it loads no modules. */
export const STOCK_CASE = 'stock';

export interface CalCase {
  id: string;
  steps: StepRef;
  /** The module ids loaded for the case, and nothing else (default: none). A subset of the profile's `modules`. */
  modules: string[];
  /** Core modules the case needs in the image (`core/sample-pool`): a target whose image lacks one skips the case, with that reason, when the runtime refuses the module. */
  needs?: string[];
  /** Played after the profile's `teardown`, also after a failure (a discovered module case gets the profile's `module_teardown`). */
  teardown?: StepRef;
}

export interface CalProfile {
  name: string;
  description: string;
  /** Seconds between starting the counters and clearing them. */
  warmup_s: number;
  capture_s: number;
  repeats: number;
  /** The modules the profile may load; the runner installs them once. */
  modules: string[];
  setup?: StepRef;
  teardown?: StepRef;
  /** The teardown of every discovered module case, after `teardown`: puts back what the module's `setup` changed (the stock machine on the track). */
  module_teardown?: StepRef;
  /** `modules`: the module cases come from the calibration files the modules ship (`dk.calcase/1`), added with `addModuleCases`. */
  discover?: 'modules';
  cases: CalCase[];
}

/** A `dk.calcase/1` file a module ships in its own directory: the case of that module in a profile that discovers module cases. */
export interface CalCaseFile {
  id: string;
  module: string;
  steps: StepRef;
  /** Core modules the case needs in the image (the module's imports from a core module that not every image has). */
  needs: string[];
}

const isTable = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

function ref(file: string, where: string, t: Record<string, unknown>): StepRef {
  if (typeof t.inline === 'string') {
    if (t.steps !== undefined || t.args !== undefined)
      throw new Error(`${file}: ${where}: inline excludes steps and args`);
    return { inline: t.inline };
  }
  if (typeof t.steps !== 'string' || t.steps === '')
    throw new Error(`${file}: ${where} needs steps = "NAME" or inline = "..."`);
  let args: Args | undefined;
  if (t.args !== undefined) {
    if (!isTable(t.args)) throw new Error(`${file}: ${where}: args must be a table`);
    args = {};
    for (const [k, v] of Object.entries(t.args)) {
      if (typeof v !== 'string' && typeof v !== 'number' && typeof v !== 'boolean')
        throw new Error(`${file}: ${where}: argument ${k} must be a string, number or boolean`);
      args[k] = v;
    }
  }
  return { steps: t.steps, ...(args ? { args } : {}) };
}

function strings(file: string, what: string, v: unknown): string[] {
  if (v === undefined) return [];
  if (!Array.isArray(v) || !v.every((x) => typeof x === 'string' && x !== ''))
    throw new Error(`${file}: ${what} must be a list of module ids`);
  return v as string[];
}

function number(file: string, doc: Record<string, unknown>, key: string, min: number, integer: boolean): number {
  const v = doc[key];
  if (typeof v !== 'number' || !(integer ? Number.isInteger(v) : Number.isFinite(v)) || v < min)
    throw new Error(`${file}: ${key} must be ${integer ? 'an integer' : 'a number'} of at least ${min}`);
  return v;
}

/** A `dk.calprofile/1` TOML file; `file` names it in error messages. Refuses a profile without the module-free case "stock". */
export function parseCalProfile(text: string, file: string): CalProfile {
  let doc: Record<string, unknown>;
  try {
    doc = parseToml(text) as Record<string, unknown>;
  } catch (e) {
    throw new Error(`${file}: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (doc.schema !== CALPROFILE_SCHEMA) throw new Error(`${file}: schema must be "${CALPROFILE_SCHEMA}"`);
  const known = [
    'schema',
    'name',
    'description',
    'warmup_s',
    'capture_s',
    'repeats',
    'modules',
    'discover',
    'setup',
    'teardown',
    'module_teardown',
    'case',
  ];
  for (const k of Object.keys(doc)) if (!known.includes(k)) throw new Error(`${file}: unknown key ${k}`);
  if (typeof doc.name !== 'string' || !/^[a-z0-9][a-z0-9-]*$/.test(doc.name))
    throw new Error(`${file}: name must be lowercase letters, digits and -`);
  const modules = strings(file, 'modules', doc.modules);
  const rawCases = doc.case;
  if (!Array.isArray(rawCases) || rawCases.length === 0) throw new Error(`${file}: it has no [[case]]`);
  const cases: CalCase[] = rawCases.map((c, i) => {
    if (!isTable(c)) throw new Error(`${file}: case ${i + 1} is not a table`);
    for (const k of Object.keys(c))
      if (!['id', 'steps', 'args', 'inline', 'modules', 'teardown'].includes(k))
        throw new Error(`${file}: case ${i + 1}: unknown key ${k}`);
    if (typeof c.id !== 'string' || !/^[a-z0-9][a-z0-9_-]*$/.test(c.id))
      throw new Error(`${file}: case ${i + 1} needs an id of lowercase letters, digits, _ and -`);
    const mods = strings(file, `case ${c.id} modules`, c.modules);
    for (const m of mods)
      if (!modules.includes(m)) throw new Error(`${file}: case ${c.id}: module ${m} is not in the profile's modules`);
    if (c.teardown !== undefined && !isTable(c.teardown))
      throw new Error(`${file}: case ${c.id}: teardown must be a table`);
    return {
      id: c.id,
      steps: ref(file, `case ${c.id}`, c),
      modules: mods,
      ...(c.teardown !== undefined
        ? { teardown: ref(file, `case ${c.id} teardown`, c.teardown as Record<string, unknown>) }
        : {}),
    };
  });
  const ids = new Set<string>();
  for (const c of cases) {
    if (ids.has(c.id)) throw new Error(`${file}: case id ${c.id} is used twice`);
    ids.add(c.id);
  }
  const stock = cases.find((c) => c.id === STOCK_CASE);
  if (!stock)
    throw new Error(
      `${file}: it needs the module-free case "${STOCK_CASE}" (the fit takes the stock load only from it)`,
    );
  if (stock.modules.length > 0) throw new Error(`${file}: the case "${STOCK_CASE}" loads no modules`);
  const out: CalProfile = {
    name: doc.name,
    description: typeof doc.description === 'string' ? doc.description : '',
    warmup_s: number(file, doc, 'warmup_s', 0, false),
    capture_s: number(file, doc, 'capture_s', 0.1, false),
    repeats: number(file, doc, 'repeats', 1, true),
    modules,
    cases,
  };
  if (doc.discover !== undefined) {
    if (doc.discover !== 'modules') throw new Error(`${file}: discover must be "modules"`);
    out.discover = 'modules';
  }
  if (doc.setup !== undefined) {
    if (!isTable(doc.setup)) throw new Error(`${file}: setup must be a table`);
    out.setup = ref(file, 'setup', doc.setup);
  }
  if (doc.teardown !== undefined) {
    if (!isTable(doc.teardown)) throw new Error(`${file}: teardown must be a table`);
    out.teardown = ref(file, 'teardown', doc.teardown);
  }
  if (doc.module_teardown !== undefined) {
    if (!isTable(doc.module_teardown)) throw new Error(`${file}: module_teardown must be a table`);
    out.module_teardown = ref(file, 'module_teardown', doc.module_teardown);
  }
  return out;
}

export const CALCASE_SCHEMA = 'dk.calcase/1';

/** A `dk.calcase/1` TOML file: `id`, `module` (the module id) and the step reference (`steps` and `args`, or `inline`). */
export function parseCalCase(text: string, file: string): CalCaseFile {
  let doc: Record<string, unknown>;
  try {
    doc = parseToml(text) as Record<string, unknown>;
  } catch (e) {
    throw new Error(`${file}: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (doc.schema !== CALCASE_SCHEMA) throw new Error(`${file}: schema must be "${CALCASE_SCHEMA}"`);
  for (const k of Object.keys(doc))
    if (!['schema', 'id', 'module', 'needs', 'steps', 'args', 'inline'].includes(k))
      throw new Error(`${file}: unknown key ${k}`);
  if (typeof doc.id !== 'string' || !/^[a-z0-9][a-z0-9_-]*$/.test(doc.id))
    throw new Error(`${file}: id must be lowercase letters, digits, _ and -`);
  if (typeof doc.module !== 'string' || doc.module === '') throw new Error(`${file}: module is missing`);
  return { id: doc.id, module: doc.module, needs: strings(file, 'needs', doc.needs), steps: ref(file, 'case', doc) };
}

/** The module teardown of a case: its step reference, with the `track` of the case's own arguments (the track the case set up), when it gives one. */
function teardownOf(t: StepRef, own: StepRef): StepRef {
  const track = 'args' in own ? own.args?.track : undefined;
  if (track === undefined || !('steps' in t)) return t;
  return { ...t, args: { ...t.args, track } };
}

/** The profile with the discovered module cases added after its own, and their modules in its module list. A profile without `discover` takes none. */
export function addModuleCases(profile: CalProfile, found: readonly CalCaseFile[]): CalProfile {
  if (profile.discover !== 'modules') return profile;
  const cases = [...profile.cases];
  const modules = [...profile.modules];
  for (const f of [...found].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))) {
    if (cases.some((c) => c.id === f.id))
      throw new Error(`calibration case ${f.id} of module ${f.module} repeats a case id`);
    if (!modules.includes(f.module)) modules.push(f.module);
    cases.push({
      id: f.id,
      steps: f.steps,
      modules: [f.module],
      ...(f.needs.length ? { needs: f.needs } : {}),
      ...(profile.module_teardown ? { teardown: teardownOf(profile.module_teardown, f.steps) } : {}),
    });
  }
  return { ...profile, cases, modules };
}

/** Canonical JSON (sorted keys) for hashing. */
export function canonicalJson(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(canonicalJson).join(',')}]`;
  if (typeof v === 'object' && v !== null)
    return `{${Object.entries(v as Record<string, unknown>)
      .filter(([, x]) => x !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
      .map(([k, x]) => `${JSON.stringify(k)}:${canonicalJson(x)}`)
      .join(',')}}`;
  return JSON.stringify(v);
}

/** Lowercase hex SHA-256 of a string (Web Crypto: Node and the browser). */
export async function sha256Hex(text: string): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(d), (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Keep the profile's declared execution order, even when callers select cases in a different order. */
export function selectCalCases(profile: CalProfile, ids: readonly string[] = []): CalProfile {
  const unknown = ids.filter((id) => !profile.cases.some((choice) => choice.id === id));
  if (unknown.length) throw new Error('Unknown calibration case ' + unknown.join(', ') + ' in ' + profile.name + '.');
  return ids.length ? { ...profile, cases: profile.cases.filter((choice) => ids.includes(choice.id)) } : profile;
}

/** Keep the INFO value as the fit key. Give hotspot lookup the full hash when the registry confirms it. */
export function captureFirmwareIdentity(infoId: string, firmwares: readonly { sha256: string }[]) {
  const id = infoId.toLowerCase();
  const match = /^[a-f0-9]{16}$/.test(id) ? firmwares.filter((firmware) => firmware.sha256.startsWith(id)) : [];
  return {
    firmware_sha256: infoId,
    ...(match.length === 1 ? { stock_firmware_sha256: match[0]!.sha256 } : {}),
  };
}
