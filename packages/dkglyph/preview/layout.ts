// Preview layout, to be replaced by measured values from the device screens. These numbers only place the mock context view.
export const SCREEN = { w: 128, h: 64, scale: 4 } as const;
/** Four parameter slots on the mock page: left edge of each slot and the top of the glyph. */
export const PAGE = { slotX: [4, 36, 68, 100], glyphTop: 14, labelTop: 40, slotW: 24 } as const;
/** Positions shown on the mock page for value glyphs, as fractions of the range. */
export const PAGE_VALUES = [0, 0.33, 0.66, 1] as const;
/** Mock menu row. */
export const MENU = { top: 46, rowH: 13, iconX: 3, textX: 18 } as const;
