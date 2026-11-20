"""A compressor for this firmware's aPLib-shaped container streams.

The stream format is the one tools/firmware/elz.py decodes and the bootstrap's depacker
accepts (docs/findings/01-container-and-patching.md): tag bits MSB first from
tag bytes fetched lazily, 1 = literal byte, 0 = match. A match is an
interlaced Elias-gamma g followed by, for g > 2, one raw byte; the offset is
((g << 8) + byte) - 767, and g == 2 repeats the last offset (initially 1).
Then two bits give a short length 1..3, or 00 and a gamma value v >= 2 for a
length of v + 2. Past offset 3328 the length gains 1, and a match copies
length + 1 bytes. The stream ends with the offset marker 767, which only the
32-bit wrap of g = 2**24 + 2, byte 0xFF reaches (the stock streams end the same
way). tools/firmware/aplib.py is the store-only encoder; this module compresses.

Method: hash chains over 4-byte keys (plus the last position of each 3-byte
key) find candidate matches; a forward dynamic program picks the cheapest
parse in bits. The cost of an edge is exact for the format; the one
approximation is the repeat offset, which each node takes from the best path
that reaches it. A match of at least `nice` bytes is taken at once and the
positions it covers are not expanded (the usual cut that keeps long zero and
0xFF runs linear). `depth` is the chain length searched per position.

Pure Python, no dependencies. On the MAIN OS section of a firmware (3.2 MB)
it needs roughly a minute; see tools/firmware/aplib_check.py for measured numbers.
"""

import struct

END_GAMMA = (1 << 24) + 2  # with byte 0xFF: (END_GAMMA << 8) + 0xFF wraps to 767
END_BYTE = 0xFF
BIAS = 767
FAR = 3328
MAX_COPIES = 1 << 20


def _gamma_cost(v):
    return 2 * (v.bit_length() - 1)


class _Bits:
    """Writes tag bits and payload bytes in the order the decoder reads them."""

    def __init__(self):
        self.buf = bytearray()
        self.tag = 0
        self.left = 0

    def bit(self, b):
        if self.left == 0:
            self.tag = len(self.buf)
            self.buf.append(0)
            self.left = 8
        self.left -= 1
        if b:
            self.buf[self.tag] |= 1 << self.left

    def byte(self, x):
        self.buf.append(x)

    def gamma(self, v):
        for ch in bin(v)[3:-1]:
            self.bit(ch == "1")
            self.bit(0)
        self.bit(bin(v)[-1] == "1")
        self.bit(1)

    def length(self, field):
        if 1 <= field <= 3:
            self.bit(field >> 1)
            self.bit(field & 1)
        else:
            self.bit(0)
            self.bit(0)
            self.gamma(field - 2)


def _len_bits(field):
    return 2 if field <= 3 else 2 + _gamma_cost(field - 2)


def _match_cost(off, copies, last):
    field = copies - 1 - (off > FAR)
    head = 3 if off == last else 1 + _gamma_cost((off + BIAS) >> 8) + 8
    return head + _len_bits(field)


def _lengths(maxlen, minlen, far):
    """The copy counts worth trying for a candidate that can copy `maxlen`.

    The length code is a step function of the count (fields 1, 2, 3 cost two
    bits; field v + 2 costs 2 + 2 * floor(log2 v) bits), so within a step only
    the longest count matters. The first few counts are all tried, since they
    decide how the parse lines up with what follows.
    """
    out = set(range(minlen, min(maxlen, minlen + 4) + 1))
    out.add(maxlen)
    k = 1
    while True:
        top = (1 << (k + 1)) + 2 + far  # copies at the top of the gamma step
        if top > maxlen:
            break
        out.add(top)
        k += 1
    out.add(min(maxlen, 4 + far))
    return sorted(c for c in out if minlen <= c <= maxlen)


def pack(data, depth=24, nice=96):
    """data -> aPLib-shaped stream (no section header), ending with the marker."""
    data = bytes(data)
    n = len(data)
    inf = 1 << 60
    cost = [inf] * (n + 1)
    prv = [0] * (n + 1)
    koff = [0] * (n + 1)  # offset of the edge that reaches the node, 0 = literal
    lastoff = [1] * (n + 1)
    cost[0] = 0
    head4 = {}
    head3 = {}
    prev4 = [-1] * (n + 1)

    def insert(i):
        if i + 4 <= n:
            k = data[i : i + 4]
            prev4[i] = head4.get(k, -1)
            head4[k] = i
        if i + 3 <= n:
            head3[data[i : i + 3]] = i

    def mlen(i, j, limit):
        """Length of the common prefix of data[i:] and data[j:], j < i."""
        m = 0
        step = 32
        while (
            m + step <= limit
            and data[i + m : i + m + step] == data[j + m : j + m + step]
        ):
            m += step
        step = 1
        while m < limit and data[i + m] == data[j + m]:
            m += step
        return m

    i = 0
    while i < n:
        c = cost[i]
        last = lastoff[i]
        limit = min(n - i, MAX_COPIES)
        # Literal.
        if c + 9 < cost[i + 1]:
            cost[i + 1] = c + 9
            prv[i + 1] = i
            koff[i + 1] = 0
            lastoff[i + 1] = last
        cands = []  # (copies, offset)
        # Repeat offset.
        if last <= i:
            m = mlen(i, i - last, limit)
            if m >= 2:
                cands.append((m, last))
        best = cands[0][0] if cands else 0
        best_off = last
        if limit >= 4:
            p = head4.get(data[i : i + 4], -1)
            walked = 0
            while p >= 0 and walked < depth:
                off = i - p
                if off != last:
                    m = mlen(i, p, limit)
                    if m > best:
                        cands.append((m, off))
                        best = m
                        best_off = off
                        if m >= nice:
                            break
                p = prev4[p]
                walked += 1
        if best < 3 and limit >= 3:
            p = head3.get(data[i : i + 3], -1)
            if p >= 0 and i - p != last and i - p <= FAR:
                m = mlen(i, p, limit)
                if m >= 3:
                    cands.append((m, i - p))
                    if m > best:
                        best = m
                        best_off = i - p
        skip = 0
        if cands:
            if best >= nice:
                # Take the longest at once; its positions are not expanded.
                m = best
                off = best_off
                j = i + m
                nc = c + _match_cost(off, m, last)
                if nc < cost[j]:
                    cost[j] = nc
                    prv[j] = i
                    koff[j] = off
                    lastoff[j] = off
                skip = m
            else:
                for m, off in cands:
                    minlen = 2 + (off > FAR)
                    for cp in _lengths(m, minlen, off > FAR):
                        j = i + cp
                        nc = c + _match_cost(off, cp, last)
                        if nc < cost[j]:
                            cost[j] = nc
                            prv[j] = i
                            koff[j] = off
                            lastoff[j] = off
        if skip:
            # Index a few positions of the covered run, then jump.
            for q in range(i, min(i + skip, i + 24)):
                insert(q)
            for q in range(max(i + 24, i + skip - 8), i + skip):
                insert(q)
            i += skip
        else:
            insert(i)
            i += 1

    # Walk the best path back, then encode forwards.
    path = []
    j = n
    while j > 0:
        pi = prv[j]
        path.append((pi, j, koff[j]))
        j = pi
    path.reverse()
    out = _Bits()
    last = 1
    for pi, j, off in path:
        if off == 0:
            out.bit(1)
            out.byte(data[pi])
            continue
        copies = j - pi
        out.bit(0)
        if off == last:
            out.gamma(2)
        else:
            raw = off + BIAS
            out.gamma(raw >> 8)
            out.byte(raw & 0xFF)
            last = off
        out.length(copies - 1 - (off > FAR))
    out.bit(0)
    out.gamma(END_GAMMA)
    out.byte(END_BYTE)
    return bytes(out.buf)


def pack_section(data, **kw):
    """pack(data) with the section header [u32 stream length][u32 byte sum]."""
    body = pack(data, **kw)
    return struct.pack(">II", len(body), sum(body) & 0xFFFFFFFF) + body


def inplace_margin(
    stream, dest=0x40000400, stage=0x40400000
):  # stage: [patch] main_stage of the device (0x40200000 on the Syntakt)
    """Slack, in bytes, of unpacking `stream` where the bootstrap does.

    The bootstrap stages a packed section at `stage` and unpacks it to `dest`
    (packages/emulator/boot/examples/depack_check.rs reproduces it). A MAIN OS image longer
    than stage - dest bytes writes over the staged stream while it is still
    being read. This returns the smallest, over the stream, of
    (stream bytes consumed) - (output bytes written past the staging address),
    both counted after each token; it is negative when a write would land on a
    stream byte not read yet. None when the output never reaches `stage`.
    """
    from tools.firmware.elz import BIAS as ELZ_BIAS
    from tools.firmware.elz import FAR as ELZ_FAR
    from tools.firmware.elz import Bits

    (length,) = struct.unpack_from(">I", stream, 0)
    s = Bits(stream, 8, 8 + length)
    room = stage - dest
    out = 0
    last = 1
    margin = None
    while True:
        if s.bit():
            s.byte()
            out += 1
        else:
            g = s.gamma()
            if g == 2:
                off = last
            else:
                raw = ((g << 8) + s.byte()) & 0xFFFFFFFF
                if raw == ELZ_BIAS:
                    break
                off = raw - ELZ_BIAS
                last = off
            short = 2 * s.bit() + s.bit()
            ln = short if short else s.gamma() + 2
            if off > ELZ_FAR:
                ln += 1
            out += ln + 1
        if out > room:
            slack = s.p - (out - room)
            margin = slack if margin is None else min(margin, slack)
    return margin
