// Which modules the site ships as prebuilt .dkm files (the Transfer page's library): the run-time content modules of modules/machine,
// modules/fx and modules/tool. The shared rule is `ModuleInfo::is_runtime`
// (packages/patchkit/src/module.rs, the desktop library's rule); the site adds its own filters (module roots, `site = false`, targets).
// Written without TS-only syntax: scripts/gen-modules.mjs (node) and data/modules.ts (vite) both import it.

export const SHIPPED_KINDS = ['machine', 'fx', 'tool'];

/** `manifest` is the parsed module.toml; `dir` the module's directory relative to the repository root, with `/`. */
export function isShipped(manifest, dir) {
  const m = manifest.module ?? {};
  if (!SHIPPED_KINDS.some((kind) => `${dir}/`.startsWith(`modules/${kind}/`))) return false;
  if (m.category === 'core' || m.type === 'framework') return false;
  // A developer tool sets `site = false`: the public site is for end users.
  if (m.site === false) return false;
  return Boolean(m.dynamic) || m.type === 'raw' || m.type === 'service';
}

/** The ids in `devices` (the devices of the site list) that the manifest does not mark unsupported (a pack that fails still leaves a device out). */
export function shippedDevices(manifest, devices) {
  const out = new Set();
  for (const [port, status] of Object.entries(manifest.targets ?? {}))
    if (status !== 'unsupported') out.add(port.split('-')[0]);
  return devices.filter((d) => out.has(d));
}
