// Which devices the public site offers: patched firmware on the Firmware page and the module gallery. This is a publication decision, not a device fact: a device is
// added here after its hardware proof, and not before. For another device the Firmware page prepares no patched image (restoring stock stays available for every
// device) and the gallery lists no module for it. The desktop workbench (apps/desktop) has no such list: it offers every device that has a `device` patch set.
// Device ids are the `[device] short` of devices/*.toml. docs/PATCHING.md "Flashing a Syntakt" says when the Syntakt may be added.
export const SITE_DEVICES: readonly string[] = ['dt2', 'dn2'];

/** Does the site offer this device? (`FirmwareEnv.offersPatched` of the site, and the gallery's device filter.) */
export const siteOffers = (device: string): boolean => SITE_DEVICES.includes(device);
