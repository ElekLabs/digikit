// The native event boundary of the browser preview: no native side emits, so a listener never fires.
export async function listen<T>(_event: string, _handler: (event: { payload: T }) => void): Promise<() => void> {
  return () => {};
}
