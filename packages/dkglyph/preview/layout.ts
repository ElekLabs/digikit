// Preview layout, measured from the stock parameter page and settings menu frames. It places the mock context view.
export const SCREEN = { w: 128, h: 64, scale: 4 } as const;
/** Four knob slots on the parameter page: glyph left edge of each slot, glyph top, slot width, label area top and label text height. */
export const PAGE = { slotX: [25, 51, 77, 103], glyphTop: 13, slotW: 17, labelTop: 32, labelH: 5 } as const;
/** Positions shown on the mock page for value glyphs, as fractions of the range. */
export const PAGE_VALUES = [0, 0.33, 0.66, 1] as const;
/** Menu row: row top, row height, selection bar (height and x range), icon left, text left. */
export const MENU = { top: 47, rowH: 13, barH: 11, barX0: 39, barX1: 119, iconX: 42, textX: 54 } as const;
