import type { DkModule } from '@digi/dk-session';
import type { ModuleChannel } from '@digi/module-ui';

export interface ModuleChannelRequest {
  /** The selected target's key (`targetKey`). */
  target: string;
  /** The loaded module record the channel serves. */
  module: DkModule;
  /** False once the instance has unloaded, the target changed or the page closed; every later call of the channel fails with `unavailable`. */
  current: () => boolean;
}

/**
 * Makes the channel of one loaded module instance. A real target gets the device channel over dk-session; the preview gets a memory channel.
 * It never touches the network when called: the channel fetches at its first request.
 */
export type ModuleChannelFactory = (request: ModuleChannelRequest) => ModuleChannel;
