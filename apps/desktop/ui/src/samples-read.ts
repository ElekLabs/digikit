/** Both panes start independently. A silent device must not delay the local listing. */
export async function readSamplePanes(
  local: () => Promise<void>,
  device: () => Promise<void>,
  errorLine: (error: unknown) => string,
): Promise<void> {
  const results = await Promise.allSettled([local(), device()]);
  const errors = results.flatMap((r) => (r.status === 'rejected' ? [errorLine(r.reason)] : []));
  if (errors.length) throw new Error(errors.join('\n'));
}
