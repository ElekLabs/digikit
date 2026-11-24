import { DkError, fileHashHex, hasCap, type DkInfoFull, type DkSession } from '@digi/dk-session';

export const SYSTEM_PACKAGE_PATH = 'digikit/system.dksp';

type PackageCommand = 'CAPACITY' | 'MODULES' | 'LOADS';

/** Package commands may be advertised even when only the resident core is running. Use one reader per INFO snapshot. */
export function systemPackageReads(info: DkInfoFull) {
  let coreOnly = false;
  return {
    get coreOnly() {
      return coreOnly;
    },
    async command<T>(name: PackageCommand, call: () => Promise<T>): Promise<T | undefined> {
      if (coreOnly || !hasCap(info, name)) return undefined;
      try {
        return await call();
      } catch (error) {
        if (
          info.safe_mode !== true ||
          !(error instanceof DkError && error.kind === 'device' && error.status === 'E_PROTO')
        )
          throw error;
        coreOnly = true;
        return undefined;
      }
    },
  };
}

/** Target-bound card operations for the trusted package bytes. */
export function systemPackageStore(
  session: Pick<DkSession, 'list' | 'verify' | 'put'>,
  bytes: Uint8Array,
  { info, check }: { info: DkInfoFull; check: () => void },
) {
  const expectedHash = fileHashHex(bytes);
  const verifyHash = async () => {
    check();
    const hash = await session.verify(SYSTEM_PACKAGE_PATH);
    check();
    return hash === expectedHash;
  };
  return {
    async verifiedOnCard(): Promise<boolean> {
      check();
      if (info.card_read_only) throw new Error('The device card store is read-only.');
      const entries = await session.list(hasCap(info, 'FOLDERS') ? SYSTEM_PACKAGE_PATH : undefined);
      const present = entries.some((entry) => entry.path === SYSTEM_PACKAGE_PATH && entry.size === bytes.length);
      check();
      if (!present) return false;
      try {
        return await verifyHash();
      } catch (error) {
        check();
        if (
          error instanceof DkError &&
          error.kind === 'device' &&
          (error.status === 'E_READ' || error.status === 'E_NOFILE')
        )
          return false;
        throw error;
      }
    },
    async put(onProgress?: (sent: number, total: number) => void): Promise<void> {
      check();
      await session.put(SYSTEM_PACKAGE_PATH, bytes, onProgress);
      check();
    },
    async verify(): Promise<void> {
      if (!(await verifyHash())) throw new Error(`Verification failed for ${SYSTEM_PACKAGE_PATH}.`);
    },
  };
}
