"""Refuse C that names a union member other than the first, for code built by Selache's C compiler (selcc).

selcc lays out union members like struct members: the first at offset 0, each later one after the previous one
(docs/findings/20-patch-toolchains.md, "selcc lays out union members like struct members"). `sizeof` is right, so a
union still holds its largest member, but `u.second`, `p->second` and a designated initialiser `.second = ...` read
and write the wrong bytes. Neither the encoding gate nor the operand cross-check can see that; only an equivalence
test on the concrete core can. pk_sharc.compile_c therefore runs `check` over every preprocessed translation unit and
refuses one that names a later member: use a pointer to the union converted to the member's type instead (C99
6.7.2.1: it points to each member; modules/machines/plaits/src/dsp/plaits.h `PL_VA`), or a plain array of bit
patterns read through a `const float *` (modules/machines/plaits/src/dsp/engines/ws_tables.h `WS_F`).

The check works on tokens, not types. It collects every union body in the unit and the names of its members after
the first (a later anonymous struct or union member contributes all its names); then it refuses every `.name` and
`->name` with such a name, wherever it appears. It fails closed: a struct field that shares its name with a later
union member is refused too (rename one of them), and so is a union it cannot parse. Attribute specifiers between
`union` and its tag or body (`__attribute__((...))`, `__declspec(...)`, `_Alignas(...)`) are skipped; a body after
more than one name (an unknown specifier, `union PACKED U { ... }`) or a `union` with neither tag nor body is refused."""

from __future__ import annotations

import re
from dataclasses import dataclass

TOKEN = re.compile(
    r"""
    (?P<ws>[ \t\r\f\v]+)
  | (?P<nl>\n)
  | (?P<comment>/\*.*?\*/|//[^\n]*)
  | (?P<pp>\#[^\n]*)
  | (?P<str>"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')
  | (?P<num>\.?\d(?:[eEpP][+-]|[\w.])*)
  | (?P<id>[A-Za-z_]\w*)
  | (?P<punct>->|\.\.\.|<<=|>>=|[-+*/%&|^!<>=]=|&&|\|\||\+\+|--|<<|>>|.)
    """,
    re.VERBOSE | re.DOTALL,
)
KEYWORDS = frozenset(
    [
        "auto",
        "break",
        "case",
        "char",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "float",
        "for",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "register",
        "restrict",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "unsigned",
        "void",
        "volatile",
        "while",
        "_Bool",
        "_Complex",
        "_Imaginary",
    ]
)


@dataclass(frozen=True)
class Tok:
    kind: str
    text: str
    line: int


@dataclass(frozen=True)
class Violation:
    line: int
    member: str
    text: str

    def __str__(self) -> str:
        if self.member == UNPARSED:
            return f"line {self.line}: `{self.text.strip()}`: a union this check cannot parse (refused)"
        return f"line {self.line}: `{self.text.strip()}` names union member `{self.member}`, not its first member"


def tokens(source: str) -> list[Tok]:
    out: list[Tok] = []
    line = 1
    for m in TOKEN.finditer(source):
        kind = m.lastgroup or "punct"
        text = m.group()
        if kind in ("id", "num", "str", "punct"):
            out.append(Tok(kind, text, line))
        line += text.count("\n")
    return out


def _close(toks: list[Tok], i: int) -> int:
    """Index of the bracket closing toks[i] (one of ( [ {)."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[toks[i].text]]
    j = i + 1
    while j < len(toks):
        t = toks[j].text
        if t in pairs:
            stack.append(pairs[t])
        elif stack and t == stack[-1]:
            stack.pop()
            if not stack:
                return j
        j += 1
    raise ValueError(f"line {toks[i].line}: unbalanced {toks[i].text!r}")


def _split(toks: list[Tok], sep: str) -> list[list[Tok]]:
    """toks split at `sep` outside any bracket."""
    parts: list[list[Tok]] = [[]]
    i = 0
    while i < len(toks):
        t = toks[i]
        if t.text in "([{" and t.kind == "punct":
            j = _close(toks, i)
            parts[-1].extend(toks[i : j + 1])
            i = j + 1
            continue
        if t.text == sep and t.kind == "punct":
            parts.append([])
        else:
            parts[-1].append(t)
        i += 1
    return [p for p in parts if p]


def _declarator_name(decl: list[Tok]) -> str | None:
    """The name a declarator (with its leading type, for the first one of a declaration) declares."""
    # a bit-field width is not part of the name
    for k, t in enumerate(decl):
        if t.text == ":" and t.kind == "punct":
            decl = decl[:k]
            break
    # function pointer or parenthesised declarator: the name inside the first ( * ... )
    for k, t in enumerate(decl):
        if t.text == "(" and k + 1 < len(decl) and decl[k + 1].text == "*":
            inner = decl[k + 1 : _close(decl, k)]
            names = [x.text for x in inner if x.kind == "id" and x.text not in KEYWORDS]
            return names[0] if names else None
    flat: list[Tok] = []
    i = 0
    while i < len(decl):
        if decl[i].text == "[" and decl[i].kind == "punct":
            i = _close(decl, i) + 1
            continue
        flat.append(decl[i])
        i += 1
    names = [x.text for x in flat if x.kind == "id" and x.text not in KEYWORDS]
    return names[-1] if names else None


def members(body: list[Tok]) -> list[list[str]]:
    """The members of a struct or union body, in order: one list of names per member (an anonymous struct or
    union member gives all of its names)."""
    out: list[list[str]] = []
    for decl in _split(body, ";"):
        brace = next(
            (k for k, t in enumerate(decl) if t.text == "{" and t.kind == "punct"), None
        )
        if brace is not None:
            end = _close(decl, brace)
            after = decl[end + 1 :]
            if not [t for t in after if t.kind == "id"]:
                out.append(
                    [n for names in members(decl[brace + 1 : end]) for n in names]
                )
                continue
            # `struct { ... } a, b[2];`: the declarators follow the body
            for d in _split(after, ","):
                name = _declarator_name(d)
                if name:
                    out.append([name])
            continue
        for d in _split(decl, ","):
            name = _declarator_name(d)
            if name:
                out.append([name])
    return out


# Specifiers that may stand between `union` and its tag or body, each with a parenthesised argument
# (`union __attribute__((aligned(8))) U { ... }`): skipped with their parentheses.
ATTRIBUTES = frozenset(
    ["__attribute__", "__attribute", "__declspec", "_Alignas", "alignas"]
)
UNPARSED = "<union>"


def _union_body(toks: list[Tok], i: int) -> int | None:
    """For `union` at toks[i]: the index of its body's `{` when this is a definition, None when it is a use of a
    union type (`union U *p`, `union U f(void)`, `(union U)`). Raises ValueError for a form it cannot tell (fail
    closed): a body after more than one name (`union PACKED U { ... }`), or no tag and no body."""
    names = 0
    j = i + 1
    while j < len(toks):
        t = toks[j]
        if t.kind == "id" and t.text in ATTRIBUTES:
            if j + 1 < len(toks) and toks[j + 1].text == "(":
                j = _close(toks, j + 1) + 1
                continue
            raise ValueError(
                f"line {t.line}: `{t.text}` without its argument after `union`"
            )
        if t.kind == "id" and t.text not in KEYWORDS:
            names += 1
            j += 1
            continue
        if t.kind == "id":  # const, volatile, ...: a use (`union U const *p`)
            break
        if t.text == "{":
            if names <= 1:
                return j
            raise ValueError(
                f"line {t.line}: a union body after {names} names (an unknown specifier?)"
            )
        break
    if names == 0:
        raise ValueError(f"line {toks[i].line}: `union` with neither a tag nor a body")
    return None


def later_members(toks: list[Tok]) -> dict[str, int]:
    """Every name that is a member after the first of some union in the unit -> the line of that union. A union
    form _union_body cannot tell is reported as the name UNPARSED."""
    out: dict[str, int] = {}
    for i, t in enumerate(toks):
        if t.kind != "id" or t.text != "union":
            continue
        try:
            j = _union_body(toks, i)
        except ValueError:
            out.setdefault(UNPARSED, t.line)
            continue
        if j is None:
            continue  # a use of a union type, not its definition
        end = _close(toks, j)
        for names in members(toks[j + 1 : end])[1:]:
            for n in names:
                out.setdefault(n, t.line)
    return out


def check(source: str) -> list[Violation]:
    """The places in `source` (a preprocessed C translation unit) that name a union member other than the first,
    and every union written in a form this check cannot parse (it fails closed)."""
    toks = tokens(source)
    later = later_members(toks)
    if not later:
        return []
    lines = source.split("\n")
    out = []
    if UNPARSED in later:
        line = later.pop(UNPARSED)
        out.append(
            Violation(line, UNPARSED, lines[line - 1] if line <= len(lines) else "")
        )
    for a, b in zip(toks, toks[1:], strict=False):
        if (
            a.kind == "punct"
            and a.text in (".", "->")
            and b.kind == "id"
            and b.text in later
        ):
            out.append(
                Violation(
                    b.line, b.text, lines[b.line - 1] if b.line <= len(lines) else ""
                )
            )
    return out


def message(violations: list[Violation], name: str = "C source") -> str:
    head = (
        f"{name}: names a union member other than the first ({len(violations)} place(s)). selcc places union "
        "members like struct members (docs/findings/20-patch-toolchains.md), so this reads the wrong bytes; use a "
        "pointer to the union converted to the member's type (modules/machines/plaits/src/dsp/plaits.h PL_VA) or a "
        "bit-pattern array read through a const float * (tools/patchkit/pk_cunion.py)"
    )
    return "\n".join([head, *(f"  {v}" for v in violations)])
