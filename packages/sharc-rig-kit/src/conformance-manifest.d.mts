export const CONFORMANCE_ROOTS: string[];
export const NEGATIVE_VARIANTS: Record<string, [string, string, string, string, string]>;
export function dspSourcePaths(manifest: Record<string, unknown>, device: string): string[];
export function conformanceKind(manifest: Record<string, unknown>): 'machine' | 'fx' | null;
export function negativeVariantsOf(id: string): string[];
