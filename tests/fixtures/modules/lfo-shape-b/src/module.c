/* test/lfo-shape-b: the same module as test/lfo-shape with the name STEP and 32 samples of two levels, -800 for the first half and 800 for the second. The module id differs, so the shape key does. */
#define SHAPE_NAME "STEP"
#define SHAPE_SAMPLE(i) ((i) < 16 ? -800 : 800)
#include "../../lfo-shape/src/module.c"
