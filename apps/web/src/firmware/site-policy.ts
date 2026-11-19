// Which devices' patched firmware the public site offers. This is a publication decision, not a device fact: a device is added here after its hardware proof,
// and not before. The site's Firmware page prepares no patched image for another device; restoring stock stays available for every device. The desktop
// workbench (apps/desktop) has no such list: it offers every device that has a `device` patch set. Device ids are the `[device] short` of devices/*.toml.
// docs/PATCHING.md "Flashing a Syntakt, in two steps" says when the Syntakt may be added.
export const SITE_PATCHED_DEVICES: readonly string[] = ['dt2', 'dn2'];

/** `FirmwareEnv.offersPatched` of the site. */
export const siteOffersPatched = (device: string): boolean => SITE_PATCHED_DEVICES.includes(device);
