import type { QueryError } from '@digi/module-ui';

/** The number the device shows for a slot: slot + 1 in three digits. */
export const slotNumber = (slot: number) => String(slot + 1).padStart(3, '0');

/** A project's line in the list: its number and name. */
export const projectTitle = (slot: number, name: string) => (name ? `${slotNumber(slot)}  ${name}` : slotNumber(slot));

/** Why a query gave no reply, in words for the user. */
export function queryErrorText(error: QueryError): string {
  switch (error.kind) {
    case 'busy':
    case 'timeout':
      return 'The device did not answer. Select the project again.';
    case 'unavailable':
      return 'The module is not available.';
    case 'refused':
      return `The module refused the request (code ${error.code}).`;
    default:
      return `The module could not answer (${error.kind}).`;
  }
}

/** The save stamp and the module's store hash as the eight hex digits they are. */
export const hex32 = (value: number) => value.toString(16).padStart(8, '0').toUpperCase();

/** How a saved value shows: its text when every shown byte is printable ASCII, else the bytes as hex pairs; `truncated` when the module's value is longer than
 * the bytes the device gave. */
export interface ValueView {
  text: string;
  hex: boolean;
  truncated: boolean;
}

export function valueView(shown: Uint8Array, length: number): ValueView {
  const printable = shown.every((byte) => byte >= 0x20 && byte <= 0x7e);
  const text = printable
    ? String.fromCharCode(...shown)
    : [...shown].map((byte) => byte.toString(16).padStart(2, '0').toUpperCase()).join(' ');
  return { text, hex: !printable, truncated: length > shown.length };
}
