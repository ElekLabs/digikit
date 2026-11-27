import type { ModuleChannel, ModuleFileEntry } from './module-channel.ts';

/** The files in one module's data directory on the target. A page reaches no other file. */
export interface ModuleFileClient {
  list(): Promise<ModuleFileEntry[]>;
  read(name: string): Promise<Uint8Array>;
  send(name: string, bytes: Uint8Array, progress: (sent: number, total: number) => void): Promise<void>;
  remove(name: string): Promise<void>;
}

/** The page-side files helper: it turns the channel's progress events back into the `progress` callback of one `send`. */
export class ChannelFiles implements ModuleFileClient {
  readonly #channel: ModuleChannel;
  #jobs = 0;
  constructor(channel: ModuleChannel) {
    this.#channel = channel;
  }

  list() {
    return this.#channel.listFiles();
  }

  read(name: string) {
    return this.#channel.readFile(name);
  }

  remove(name: string) {
    return this.#channel.removeFile(name);
  }

  async send(name: string, bytes: Uint8Array, progress: (sent: number, total: number) => void) {
    const job = ++this.#jobs;
    const stop = this.#channel.subscribe((event) => {
      if (event.kind === 'file-progress' && event.job === job) progress(event.done, event.total);
    });
    try {
      await this.#channel.sendFile(job, name, bytes);
    } finally {
      stop();
    }
  }
}
