/** An AggregateError whose message names its causes (Vitest prints only the message). */
export function aggregate(errors: unknown[], what: string): AggregateError {
  const causes = errors.map((e) => (e instanceof Error ? e.message : String(e))).join(' | ');
  return new AggregateError(errors, `${what}: ${causes}`);
}
