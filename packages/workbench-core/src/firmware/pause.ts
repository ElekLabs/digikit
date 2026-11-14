// A pause for the sender's 2 ms. `setTimeout` is not made for it: after a few nested timers a browser waits at least 4 ms, which would double the
// time of a send. A short pause yields to the event loop through a message channel until the time has passed, which has no such floor.

const channel = typeof MessageChannel === 'undefined' ? undefined : new MessageChannel();
let waiting: (() => void) | undefined;
if (channel) {
  channel.port1.onmessage = () => {
    const w = waiting;
    waiting = undefined;
    w?.();
  };
}

const yieldNow = (): Promise<void> =>
  new Promise((resolve) => {
    waiting = resolve;
    channel!.port2.postMessage(0);
  });

/** Wait `ms` milliseconds; one pause at a time. A pause of 8 ms or more is a plain timer. */
export async function pause(ms: number): Promise<void> {
  if (!channel || ms >= 8) {
    await new Promise<void>((resolve) => setTimeout(resolve, ms));
    return;
  }
  const end = performance.now() + ms;
  do await yieldNow();
  while (performance.now() < end);
}
