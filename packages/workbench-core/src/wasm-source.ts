// Where the apps get a wasm file: `WasmSource.bytes(name)`. The desktop reads them through the Host (the Tauri commands that give the files
// of the checkout); the site fetches them from its own base URL; a test gives bytes it made or a stub of what the wasm does (`PatchKitLike`,
// `NeedsLike`). Names: `patchkit.wasm`, `emulator-core-sharc.wasm`, `dksample.wasm` (desktop only).

export interface WasmSource {
  /** The bytes of the file; a failed load throws (the caller tries again at its next use). */
  bytes(name: string): Promise<ArrayBuffer>;
  /** The compiled module, where the source can compile while it downloads (`WebAssembly.compileStreaming`); callers fall back to `compile`. */
  module?(name: string): Promise<WebAssembly.Module>;
}

/** The module of `name`: the source's own, or compiled from its bytes. */
export async function wasmModule(source: WasmSource, name: string): Promise<WebAssembly.Module> {
  return source.module ? source.module(name) : WebAssembly.compile(await source.bytes(name));
}

/** The site's files, fetched from `base` (the site's BASE_URL with a trailing slash). */
export function fetchWasmSource(base: string): WasmSource {
  const get = async (name: string): Promise<Response> => {
    const r = await fetch(`${base}${name}`);
    if (!r.ok) throw new Error(`patch tool files are missing from this site (${name}: ${r.status})`);
    return r;
  };
  return {
    bytes: async (name) => (await get(name)).arrayBuffer(),
    module: async (name) => WebAssembly.compileStreaming(get(name)),
  };
}

/** Files held in memory (tests): `files` by name. A name that is not there throws. */
export function memoryWasmSource(
  files: Record<string, ArrayBuffer | Uint8Array> = {},
): WasmSource & { asked: string[] } {
  const asked: string[] = [];
  return {
    asked,
    async bytes(name) {
      asked.push(name);
      const f = files[name];
      if (!f) throw new Error(`memoryWasmSource: no file ${name}`);
      return f instanceof Uint8Array ? (f.slice().buffer as ArrayBuffer) : f;
    },
  };
}

/** The 8 bytes of an empty, valid wasm module: compiles, exports nothing. */
export const EMPTY_WASM = new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]);
