// The OLED frame of the device screen: 128 x 64 pixels in 1024 bytes. Pixel (x, y) is bit y%8 of byte (7 - y/8) + 8*x.
// The one owner of this layout for the TypeScript side: the PNG writer of the tests (packages/test-support) and the image
// reader of the desktop preview (apps/desktop/ui/preview/frame-from-png.ts) both go through it.
export const OLED_WIDTH = 128;
export const OLED_HEIGHT = 64;
export const OLED_FRAME_BYTES = (OLED_WIDTH * OLED_HEIGHT) / 8;

/** Whether pixel (x, y) is lit in `frame` (OLED_FRAME_BYTES bytes). */
export function oledPixel(frame: Uint8Array, x: number, y: number): boolean {
  return ((frame[7 - (y >> 3) + 8 * x]! >> (y & 7)) & 1) === 1;
}

/** Lights pixel (x, y) in `frame`. */
export function setOledPixel(frame: Uint8Array, x: number, y: number): void {
  frame[7 - (y >> 3) + 8 * x]! |= 1 << (y & 7);
}
