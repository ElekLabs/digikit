// Values kept per browser profile of the app (localStorage). Storage can be unavailable: a read gives null, a write lasts until the window closes.
export function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
export function writePref(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* the value lasts until the window closes */
  }
}
