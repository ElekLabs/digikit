import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { parse } from 'smol-toml';
import { renderGlyph, type RenderedGlyph, type SizeClasses } from './frames.ts';
import { type GlyphDef, glyph } from './glyph.ts';

export interface CatalogueEntry {
  id: string;
  group: string;
  v1: boolean;
  note: string;
}

export interface GlyphError {
  message: string;
  /** the first stack line inside the glyph file, when there is one */
  where: string;
}

export interface LoadedGlyph {
  id: string;
  group: string;
  file: string;
  def?: GlyphDef;
  rendered?: RenderedGlyph;
  error?: GlyphError;
}

export interface LoadedSet {
  dir: string;
  name: string;
  title: string;
  description: string;
  sizes: SizeClasses;
  /** empty outer ring in px per size class; a missing class keeps 1 */
  margins: Readonly<Record<string, number>>;
  catalogue: CatalogueEntry[];
  glyphs: LoadedGlyph[];
}

const SUFFIX = '.glyph.mjs';

function readToml(file: string): Record<string, unknown> {
  try {
    return parse(readFileSync(file, 'utf8')) as Record<string, unknown>;
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return {};
    throw new Error(`${path.basename(file)}: ${(e as Error).message}`);
  }
}

function glyphFiles(dir: string, rel = ''): string[] {
  const out: string[] = [];
  for (const e of readdirSync(path.join(dir, rel), { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...glyphFiles(dir, r));
    else if (e.name.endsWith(SUFFIX)) out.push(r);
  }
  return out;
}

function errorOf(e: unknown, file: string): GlyphError {
  const err = e instanceof Error ? e : new Error(String(e));
  const line = (err.stack ?? '').split('\n').find((l) => l.includes(SUFFIX));
  const m = line?.match(/([^/\\\s(]+\.glyph\.mjs)(?:\?[^:\s)]*)?:(\d+(?::\d+)?)/);
  const where = m ? `${m[1]}:${m[2]}` : (line?.trim() ?? path.basename(file));
  return { message: err.message, where };
}

async function loadGlyph(dir: string, rel: string, sizes: SizeClasses): Promise<LoadedGlyph> {
  const id = rel.slice(0, -SUFFIX.length);
  const file = path.join(dir, rel);
  const out: LoadedGlyph = { id, group: id.split('/')[0]!, file };
  try {
    // The query only refreshes this file; helpers it imports stay cached, so they are fresh only in a new process.
    const mod = await import(`${pathToFileURL(file).href}?v=${statSync(file).mtimeMs}`);
    if (!mod.default) throw new Error('the file must export a glyph definition as default');
    out.def = glyph(mod.default as GlyphDef);
    out.rendered = renderGlyph(out.def, sizes);
  } catch (e) {
    out.error = errorOf(e, file);
  }
  return out;
}

/** Load a set directory. A broken glyph file becomes an error entry; the rest of the set loads. */
export async function loadSet(dir: string): Promise<LoadedSet> {
  const abs = path.resolve(dir);
  const meta = readToml(path.join(abs, 'set.toml'));
  const sizes: Record<string, readonly [number, number]> = {};
  for (const [k, v] of Object.entries((meta.sizes as Record<string, unknown>) ?? {}))
    if (Array.isArray(v) && v.length === 2) sizes[k] = [Number(v[0]), Number(v[1])];
  const margins: Record<string, number> = {};
  for (const [k, v] of Object.entries((meta.margins as Record<string, unknown>) ?? {}))
    if (Number.isInteger(Number(v)) && Number(v) >= 0) margins[k] = Number(v);
  const cat = readToml(path.join(abs, 'catalogue.toml'));
  const catalogue = ((cat.glyph as Array<Record<string, unknown>>) ?? []).map((g) => ({
    id: String(g.id),
    group: String(g.group ?? String(g.id).split('/')[0]),
    v1: g.v1 === true,
    note: String(g.note ?? ''),
  }));
  const glyphs: LoadedGlyph[] = [];
  for (const rel of glyphFiles(abs)) glyphs.push(await loadGlyph(abs, rel, sizes));
  return {
    dir: abs,
    name: String(meta.name ?? path.basename(abs)),
    title: String(meta.title ?? meta.name ?? path.basename(abs)),
    description: String(meta.description ?? ''),
    sizes,
    margins,
    catalogue,
    glyphs,
  };
}
