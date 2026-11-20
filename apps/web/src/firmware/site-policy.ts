// Which devices the public site offers: patched firmware on the Firmware page, the module gallery, the emulator pick, the Transfer library, and every place the site
// lists or names a device. This is a publication decision, not a device fact: a device is added to SITE_DEVICES after its hardware proof, and not before; that is the
// only change a new device needs on the site. For another device the Firmware page prepares no patched image (restoring stock stays available for every device) and
// the pages list no module for it. The desktop workbench (apps/desktop) has no such list: it offers every device that has a `device` patch set.
// Device ids are the `[device] short` of devices/*.toml. docs/PATCHING.md "Flashing a Syntakt" says when the Syntakt may be added.
import { deviceNamesText, newestReleasesText, runtimeDeviceIds } from '@digi/workbench-core/data/device-profiles.ts';
import { runtimeFaceplateIds } from '@digi/workbench-core/data/faceplates.ts';

import { SITE_DEVICES as LIST } from '../data/site-devices.mjs';

/** The devices the site offers (src/data/site-devices.mjs: the list the site build reads too). */
export const SITE_DEVICES: readonly string[] = LIST;

/** Does the site offer this device? (`FirmwareEnv.offersPatched` of the site.) */
export const siteOffers = (device: string, offered: readonly string[] = SITE_DEVICES): boolean =>
  offered.includes(device);

/** The runtime devices that the site offers, in registry order: the one list the site's pages, gallery filters and environments use. */
export const siteDevices = (offered: readonly string[] = SITE_DEVICES): readonly string[] =>
  runtimeDeviceIds.filter((d) => offered.includes(d));

/** The site's devices that have a faceplate (the emulator's device pick). */
export const siteFaceplateIds = (offered: readonly string[] = SITE_DEVICES): readonly string[] =>
  runtimeFaceplateIds.filter((d) => offered.includes(d));

/** The site's device names as prose: "A and B" or "A or B". */
export const siteDeviceNamesText = (conj: 'and' | 'or', offered: readonly string[] = SITE_DEVICES): string =>
  deviceNamesText(conj, siteDevices(offered));

/** The newest releases of the site's devices as prose. */
export const siteNewestReleasesText = (offered: readonly string[] = SITE_DEVICES): string =>
  newestReleasesText(siteDevices(offered));
