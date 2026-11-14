// The apps reach their TransferApp / FirmwareApp through Solid context: the entry point (the desktop's main.tsx, the site's islands) wraps its
// pages in the providers (`<AppProvider app={…}>` in AppProvider.tsx) and a component asks `useTransferApp()`.
import { createContext, useContext } from 'solid-js';
import type { FirmwareApp } from '../firmware/app';
import type { TransferApp } from './app';

export const TransferAppContext = createContext<TransferApp>();
export const FirmwareAppContext = createContext<FirmwareApp>();

export function useTransferApp(): TransferApp {
  const app = useContext(TransferAppContext);
  if (!app) throw new Error('No TransferApp: wrap the page in <AppProvider>.');
  return app;
}

export function useFirmwareApp(): FirmwareApp {
  const app = useContext(FirmwareAppContext);
  if (!app) throw new Error('No FirmwareApp: wrap the page in <AppProvider firmware={…}>.');
  return app;
}
