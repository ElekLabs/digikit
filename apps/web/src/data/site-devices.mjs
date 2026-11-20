// The devices the public site offers: the one list. Everything public follows it: the site build (scripts/gen-modules.mjs: which modules are packed and listed
// per device), the pages, the gallery, the Firmware, Transfer and emulator pages (src/firmware/site-policy.ts derives their lists from it).
// A publication decision, not a device fact: a device is added after its hardware proof, and not before. Device ids are the `[device] short` of devices/*.toml.
// docs/PATCHING.md "Flashing a Syntakt" says when the Syntakt may be added. Written without TS-only syntax: node scripts and vite both import it.
export const SITE_DEVICES = ['dt2', 'dn2'];
