/* test/lfo-shape-b: the same module as test/lfo-shape with the name STEP and 32 samples of two levels, -24576 for the first half and 24576 for the second. The module id differs, so the shape key does. */
#define SHAPE_NAME "STEP"
#define SHAPE_SAMPLE(i) ((i) < 16 ? -24576 : 24576)
#include "../../lfo-shape/src/module.c"
