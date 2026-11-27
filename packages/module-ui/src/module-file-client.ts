/** The files in one module's data directory on the target. A page reaches no other file. */
export interface ModuleFileClient {
  list(): Promise<{ name: string; size: number }[]>;
  read(name: string): Promise<Uint8Array>;
  send(name: string, bytes: Uint8Array, progress: (sent: number, total: number) => void): Promise<void>;
  remove(name: string): Promise<void>;
}
