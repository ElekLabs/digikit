import { fmtVersion } from '@digi/dk-session';
import { asked, noteConnection } from '../../../web/src/transfer/connection-state';
import { session as running, target } from '../../../web/src/transfer/target';
import { transferEnv } from '../../../web/src/transfer/env';
import { setDeviceInfo } from '../../../web/src/transfer/transfer-state';
import { ACTIVE_BOOTSCREEN } from './bootscreens-core';

export async function openBootscreens() {
  const current = target();
  const isCurrent = () => current === target();
  const s = await asked(running, isCurrent);
  const info = await asked(() => s.info(), isCurrent);
  const name = await transferEnv().deviceName(info.port);
  const runtime = fmtVersion(info.runtime);
  if (current === target()) noteConnection({ type: 'answered', device: { id: info.port, name, runtime } });
  return { s, name, runtime };
}

export async function readActiveBootscreen(): Promise<{ size: number } | undefined> {
  const current = target();
  const { s, name, runtime } = await openBootscreens();
  const entries = await asked(
    () => s.list(),
    () => current === target(),
  );
  if (current === target()) setDeviceInfo({ name, runtime });
  return entries.find((e) => e.path === ACTIVE_BOOTSCREEN);
}
