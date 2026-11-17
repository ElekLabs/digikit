// Gives a page its apps (app-context.ts). `firmware` only where the page has the Firmware parts.
import type { JSX } from 'solid-js';
import type { FirmwareApp } from '../../src/firmware/app';
import type { TransferApp } from '../../src/transfer/app';
import { FirmwareAppContext, TransferAppContext } from '../../src/transfer/app-context';

export default function AppProvider(props: {
  app: TransferApp;
  firmware?: FirmwareApp;
  children: JSX.Element;
}): JSX.Element {
  // the apps are built once at startup and never change
  /* oxlint-disable solid/reactivity */
  return (
    <TransferAppContext.Provider value={props.app}>
      <FirmwareAppContext.Provider value={props.firmware}>{props.children}</FirmwareAppContext.Provider>
    </TransferAppContext.Provider>
  );
  /* oxlint-enable solid/reactivity */
}
