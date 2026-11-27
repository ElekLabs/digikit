# test/c-demo

A test fixture for C modules (`docs/findings/35-c-modules-coldfire-runtime.md`), not for players. `init` computes 64-bit
quotients, remainders, a product and three shifts from inputs the compiler cannot fold, stores them in `c_demo_out` and logs the
address. Every operation is a libgcc helper call on a 32-bit ColdFire; the helpers come from the `-mcpu=54418` libgcc that
`patchkit build` links in, as local symbols. `uv run python -m tools.patchkit.pk_cdemo proof --image dt2-1.17` boots with the packed module
selected, so the runtime loads it, and compares the stored words with Python's.
