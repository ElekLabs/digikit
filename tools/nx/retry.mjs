// Retry policy of device targets: a file that failed only with a load symptom runs once more.
// Used by tools/devtools/emu-slot.mjs, which wraps every device and conformance command and holds the slot.
// The symptoms are the ones that an overloaded emulator slot produced and that passed when rerun alone.
// A retry is never silent: emu-slot prints it, and appends it to retries.jsonl in the gate trace dir, where
// tools/nx/gate-history.mjs counts it ("retried (n)").
export const SYMPTOMS = [
  { name: 'boot-timeout-wall', pattern: /no new boot within \d+ s/ },
  { name: 'boot-timeout-emulated', pattern: /no new boot: not seen within \d+ ms of emulated time/ },
  { name: 'lifecycle-deadline', pattern: /runtime lifecycle deadline exceeded/ },
  { name: 'connection-closed', pattern: /emulator closed the connection/ },
  { name: 'remote-release-timeout', pattern: /remote release timed out/ },
];

// A real assertion failure of a test is never retried, even if a symptom appears beside it.
const ASSERTION =
  /AssertionError|expected .* to (?:be|equal|match|contain|have|deeply|strictly|include)|Snapshot .* mismatched/;

const ANSI = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, 'g');

// Returns the symptom name when the failed run's output shows a load symptom and no assertion failure.
export function classifyFailure(output) {
  const text = String(output).replace(ANSI, '');
  if (ASSERTION.test(text)) return undefined;
  return SYMPTOMS.find((s) => s.pattern.test(text))?.name;
}

// Decision for a finished attempt (1-based). Retry only a first failed attempt with a symptom.
export function retryDecision({ status, attempt, output }) {
  if (status === 0) return { retry: false };
  if (attempt >= 2) return { retry: false, symptom: classifyFailure(output) };
  const symptom = classifyFailure(output);
  return symptom ? { retry: true, symptom } : { retry: false };
}
