// Which USB device is the one on the MIDI port (docs/plans/usb-fast.md section 5). Several Elektron devices can be attached and their USB serial strings are
// the same, so the host proves it with a nonce: it sends 8 random bytes to the selected MIDI target (SET_NONCE, a SysEx service command of the usb-fast
// module), opens each device that offers the bulk interface, asks INFO and uses only the one that returns the nonce. Used by the desktop app and by
// tools/devtools/wb.mjs (the same code). Written without TS-only runtime syntax so node can import it.
import { BulkSession, type BulkLink, type BulkOptions } from './bulk.ts';
import { BULK_NONCE_LEN } from './gen_bulk.ts';

/** A device that offers the bulk interface (the desktop: `bus:address`; the browser: an index of the devices the page may use). */
export interface BulkCandidate {
  id: string;
  name?: string;
}
/** What the app can do. */
export interface BulkOpener {
  /** The devices that offer the bulk interface now (cheap, no claim). Empty: no bulk link, stay on SysEx. */
  candidates(): Promise<BulkCandidate[]>;
  open(id: string): Promise<BulkLink>;
  /** The devices with an OUT-only bulk interface (USB AUDIO/MIDI: a single bulk OUT endpoint, vendor class 0xFF). Absent or empty: none. */
  outOnlyCandidates?(): Promise<BulkCandidate[]>;
  /** The link of an OUT-only candidate: frames go OUT, the replies come from `poll` (`SysExReplyLink`). */
  openOutOnly?(id: string, poll: () => Promise<Uint8Array>): Promise<BulkLink>;
}
/** The SysEx side: `DkSession.usbFastNonce`; `usbFastReplies` (the 'R' poll) is needed for OUT-only candidates. */
export interface NonceSender {
  usbFastNonce(nonce: Uint8Array): Promise<void>;
  usbFastReplies?(): Promise<Uint8Array>;
}

export interface BulkChoice {
  session: BulkSession;
  link: BulkLink;
  id: string;
}

/** The INFO wait per OUT-only candidate. */
const OUT_ONLY_PROBE_MS = 2000;
const same = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((x, i) => x === b[i]);
const why = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The bulk session for the device on the MIDI target, or undefined (with the reason in `note`): no candidate (not logged: usb-fast is not loaded), the
 * nonce could not be set, no candidate returned it. Every candidate that is not chosen is closed.
 */
export async function chooseBulk(
  sysex: NonceSender,
  opener: BulkOpener,
  note: (line: string) => void = () => {},
  options: BulkOptions = {},
): Promise<BulkChoice | undefined> {
  let candidates: BulkCandidate[];
  try {
    candidates = await opener.candidates();
  } catch (e) {
    note(`USB bulk link not used: listing the devices failed: ${why(e)}`);
    return undefined;
  }
  let outOnly: BulkCandidate[] = [];
  if (opener.outOnlyCandidates && opener.openOutOnly && sysex.usbFastReplies) {
    try {
      outOnly = await opener.outOnlyCandidates();
    } catch (e) {
      note(`USB OUT-only link not used: listing the devices failed: ${why(e)}`);
    }
  }
  if (candidates.length === 0 && outOnly.length === 0) return undefined;
  const nonce = new Uint8Array(BULK_NONCE_LEN);
  globalThis.crypto.getRandomValues(nonce);
  try {
    await sysex.usbFastNonce(nonce);
  } catch (e) {
    note(
      `USB bulk link not used: the device on the MIDI port did not take the nonce (usb-fast not loaded?): ${why(e)}`,
    );
    return undefined;
  }
  for (const c of candidates) {
    let link: BulkLink | undefined;
    try {
      link = await opener.open(c.id);
      const session = new BulkSession(link, options);
      if (same((await session.info()).nonce, nonce)) return { session, link, id: c.id };
      note(`USB device ${c.name ?? c.id}: not the one on the MIDI port (other nonce)`);
    } catch (e) {
      note(`USB device ${c.name ?? c.id}: ${why(e)}`);
    }
    await Promise.resolve(link?.close()).catch(() => {});
  }
  // the OUT-only candidates after the pairs: INFO goes OUT, the reply comes back over SysEx ('R' poll)
  for (const c of outOnly) {
    let link: BulkLink | undefined;
    try {
      link = await opener.openOutOnly!(c.id, () => sysex.usbFastReplies!());
      const session = new BulkSession(link, options);
      // a device that is not on the MIDI port NAKs INFO (its ring is not enabled): do not wait the full reply timeout for it
      const probeMs = Math.min(options.timeoutMs ?? OUT_ONLY_PROBE_MS, OUT_ONLY_PROBE_MS);
      if (same((await session.info(probeMs)).nonce, nonce)) return { session, link, id: c.id };
      note(`USB device ${c.name ?? c.id}: not the one on the MIDI port (other nonce)`);
    } catch (e) {
      note(`USB device ${c.name ?? c.id}: ${why(e)}`);
    }
    await Promise.resolve(link?.close()).catch(() => {});
  }
  note(
    `USB bulk link not used: none of ${candidates.length + outOnly.length} candidate(s) returned this device's nonce`,
  );
  return undefined;
}
