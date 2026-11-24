/* Byte layout of dk_page_frame on the ColdFire (4-byte pointers), shared with the machines service, which builds the frame
 * (services/machines/src/native_page/frame.rs). module.c asserts it against the struct as the compiler lays it out; the Rust
 * test `layout_header_matches_the_constants` checks these numbers against the service's constants. */
#ifndef NATIVE_PAGE_LAYOUT_H
#define NATIVE_PAGE_LAYOUT_H
#define NP_FRAME_BYTES 100
#define NP_OFF_VERSION 0
#define NP_OFF_SIZE 4
#define NP_OFF_TRACK 8
#define NP_OFF_PARAMS 12
#define NP_OFF_CANVAS_WIDTH 44
#define NP_OFF_CANVAS_HEIGHT 48
#define NP_OFF_CANVAS_TEXT 52
#define NP_OFF_CANVAS_FILL 56
#define NP_OFF_WORD_COUNT 60
#define NP_OFF_WORD_VALUES 64
#define NP_OFF_LEVEL 96
#endif
