#!/usr/bin/env python3
"""Generate the exhaustive `channel_alias_accept_*` / `channel_alias_reject_*`
fixture battery for `channel_alias`'s custom `validation=` callable
(docs/condarc_research.md §3, §4.1).

`channel_alias` is declared as a plain, single, non-tuple `str`
`element_type` (`PrimitiveParameter(DEFAULT_CHANNEL_ALIAS,
validation=channel_alias_validation)`), with no enum/nullable/sequence
shape -- so, unlike every boolish/numeric/enum key, it goes through
*neither* `boolify()` nor `numberify()` nor Enum lookup. Read straight
from `conda/base/context.py`:

```python
def channel_alias_validation(value: str) -> str | Literal[True]:
    if value and not has_scheme(value):
        return f"channel_alias value '{value}' must have scheme/protocol."
    return True
```

and `conda/common/url.py`:

```python
def has_scheme(value: str) -> bool:
    return re.match(r"[a-z][a-z0-9]{0,11}://", value)
```

Two things worth spelling out explicitly, since they're easy to get
wrong when only reading the English summary ("must have a URL scheme"):

  1. **The scheme regex is lowercase-only and ASCII-alphanumeric-only.**
     `[a-z]` (not `[A-Za-z]`) for the first character, `[a-z0-9]` (not
     `[A-Za-z0-9+.-]`, unlike the RFC 3986 `scheme` grammar) for up to
     11 more. So `"HTTPS://x"`, `"Https://x"`, and even RFC-3986-legal
     schemes containing `+`/`-`/`.` (e.g. `"git+ssh://x"`) are all
     rejected -- despite superficially "having a scheme" in the plain
     English sense. Only `re.match` (a prefix match, not `re.fullmatch`)
     is used, so anything *after* a valid `scheme://` prefix is
     irrelevant to this check.
  2. **The scheme is length-bounded: 1 + up to 11 = 12 characters total
     before `://`.** `{0,11}` on top of the mandatory first character.
     A 13th character pushes the match past what `re.match` can find
     starting at position 0, so the whole check fails -- there is no
     "trailing characters are fine" leniency the way there is *after*
     `://`.

Also, **`channel_alias` does not have whitespace stripped**, unlike
almost every other string-shaped key in this suite. `typify()`
(`conda/auxlib/type_coercion.py`) unconditionally strips string values,
but `LoadedParameter._typify_data_structure`
(`conda/common/configuration.py`) special-cases `isinstance(value, str)
and issubclass(type_hint, str)` to skip `typify()` entirely and preserve
the string byte-for-byte -- and `channel_alias`'s `element_type` is the
single, exact `str` class, so this special case applies (unlike, say,
`default_python`'s `(str, NoneType)` *tuple* element_type, which does
NOT hit this special case and *does* get whitespace-stripped -- see
`generate_default_python_condarc_fixtures.py`'s module docstring for the
contrast). Concretely: `"  https://x  "` (leading whitespace) is
**rejected** (the leading spaces push the required `[a-z]` first
character off position 0, so `re.match` never matches), while
`"https://x "` (*trailing* whitespace only) is **accepted** (the
`scheme://` prefix still starts at position 0; `re.match` doesn't care
what comes after).

Non-string JSON values (`bool`, `int`, `null`) do not error out at the
type-coercion stage the way they would for a sequence/map-typed key --
`channel_alias`'s `element_type` is a single concrete non-bool type, so
`typify()`'s dispatch table (docs/condarc_research.md §2.1's `"a single
concrete type T (not a tuple), T otherwise"` row) calls the plain
`str(value)` constructor, which never raises for any of `bool`/`int`/
`None`. The stringified result (`"True"`, `"False"`, `"123"`, `"None"`)
then reaches `channel_alias_validation` as an ordinary non-empty string
that (barring extraordinary luck) has no scheme, so all four are
rejected by the *validation* check, not by type coercion -- worth its
own fixtures specifically because the rejection mechanism differs from
every sequence/map-typed key's clean `InvalidTypeError`.

An empty string is the sole value that bypasses `has_scheme()` entirely
(`if value and not has_scheme(value)` -- an empty string is falsy, so
the `and` short-circuits) and is unconditionally valid.

The collection-shape battery (list/dict raw values, empty vs. non-empty)
mirrors every other `PrimitiveParameter`-backed key's crash/clean-
rejection boundary (docs/condarc_research.md §8 item 7): an *empty*
list/dict fails cleanly with `InvalidTypeError`; a *non-empty* one
crashes with an unhandled `AttributeError` (`'YamlRawParameter' object
has no attribute 'typify'`). Both are still correctly `invalid/` either
way.

Every candidate (both batteries) is verified empirically against a real
`conda` installation before a fixture is written -- an accept candidate
that's unexpectedly *rejected*, or a reject candidate that's
unexpectedly *accepted*, is reported as SKIPPED rather than silently
written to the wrong directory, so this script is self-correcting if
conda's behavior ever changes.

Usage:
    python3 scripts/generate_channel_alias_condarc_fixtures.py

Requires a Python interpreter with `conda` importable. Resolution order
matches `tests/condarc_conformance.rs`'s `find_conda_python`:
$ALLEZ_CONFORMANCE_PYTHON, then `python3` on PATH, then the interpreter
shipped alongside whatever `conda` executable is on PATH.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VALID_DIR = REPO_ROOT / "conformance" / "condarc" / "valid"
INVALID_DIR = REPO_ROOT / "conformance" / "condarc" / "invalid"

KEY = "channel_alias"

# (slug, value) accept candidates -- see module docstring for the exact
# `has_scheme()` regex (`^[a-z][a-z0-9]{0,11}://`) and the "no whitespace
# stripping" nuance this battery is built to exercise.
ACCEPT_CANDIDATES: list[tuple[str, object]] = [
    # The only value that bypasses has_scheme() entirely: `if value and
    # not has_scheme(value)` short-circuits on an empty (falsy) string.
    ("empty_string_bypasses_check", ""),
    # Shortest possible legal scheme: exactly one lowercase letter.
    ("scheme_single_letter", "a://x"),
    # A scheme containing a digit after the mandatory leading letter --
    # `[a-z0-9]` for the remaining (up to 11) characters.
    ("scheme_alnum_with_digit", "a1://x"),
    # Exactly 12 characters before `://` (1 mandatory + 11 optional) --
    # the upper boundary the `{0,11}` quantifier still matches.
    ("scheme_length_boundary_12_chars", "abcdefghijkl://x"),
    # conda's own real-world default value (DEFAULT_CHANNEL_ALIAS).
    ("canonical_default_value", "https://conda.anaconda.org"),
    ("scheme_http", "http://example.com"),
    ("scheme_file_triple_slash", "file:///tmp"),
    ("scheme_s3", "s3://bucket/prefix"),
    # A scheme with nothing else after it -- has_scheme() only checks
    # the prefix, so an empty "rest of the URL" is still accepted.
    ("scheme_only_no_path", "https://"),
    # Trailing whitespace does NOT get stripped (see module docstring),
    # but has_scheme()'s re.match only cares about what starts at
    # position 0 -- the scheme prefix still matches regardless of what
    # comes after it, trailing whitespace included.
    ("scheme_with_trailing_whitespace", "https://x "),
]

# (slug, value) reject candidates.
REJECT_CANDIDATES: list[tuple[str, object]] = [
    # 13 characters before `://` -- one past the {0,11} boundary, so
    # re.match can no longer find a valid scheme starting at position 0.
    ("scheme_too_long_13_chars", "abcdefghijklm://x"),
    # The mandatory first character must be `[a-z]`, not a digit.
    ("scheme_starts_with_digit", "1abc://x"),
    # Fully uppercase scheme -- has_scheme()'s regex is lowercase-only.
    ("scheme_uppercase", "HTTPS://x"),
    # Only the first character uppercase -- still fails the same
    # lowercase-only `[a-z]` check (case sensitivity, not an all-or-
    # nothing casing rule).
    ("scheme_mixedcase_titlecase", "Https://x"),
    # No `://` anywhere -- has_scheme() finds nothing to match at all.
    ("string_no_scheme_plain_text", "no-scheme-here"),
    # A colon but no double-slash -- `://` is a literal 3-character
    # requirement, not just "has a colon".
    ("string_colon_no_double_slash", "https:x"),
    # An empty scheme name before `://` -- the mandatory `[a-z]` first
    # character has nothing to match.
    ("string_empty_scheme", "://x"),
    # `+` is legal in the RFC 3986 `scheme` grammar (e.g. real-world
    # `git+ssh://`) but NOT in conda's narrower `[a-z0-9]`-only regex --
    # demonstrates this check is stricter than the URL spec it's
    # nominally modeling.
    ("scheme_disallowed_char_plus", "a+b://x"),
    # Leading whitespace is NOT stripped for this key (see module
    # docstring) -- it pushes the `[a-z]` first-character requirement
    # off position 0, so re.match fails even though the rest of the
    # string is a perfectly valid scheme.
    ("string_leading_whitespace_breaks_match", "  https://x  "),
    # A bare JSON null coerces to the *string* "None" (str(None)) via
    # typify()'s plain-`T(value)` dispatch for a single, non-bool
    # element_type -- which then fails has_scheme() like any other
    # ordinary schemeless string, not via a type-coercion error.
    ("null_literal_coerces_to_string", None),
    # Likewise, bool/int values are stringified (str(True) == "True",
    # str(123) == "123") rather than rejected at the type-coercion
    # stage -- rejection happens in channel_alias_validation itself.
    ("bool_true_coerces_to_string", True),
    ("bool_false_coerces_to_string", False),
    ("int_value_coerces_to_string", 123),
    # Collection-shape battery (docs/condarc_research.md §8 item 7):
    # empty list/dict fails cleanly; non-empty list/dict crashes with
    # an unhandled AttributeError. Both are still correctly invalid.
    ("array_empty", []),
    ("array_nonempty", [1, 2]),
    ("object_empty", {}),
    ("object_nonempty", {"a": 1}),
]

CONDA_CHECK_SCRIPT = """
import sys
from conda.base.context import reset_context, context

path = sys.argv[1]
try:
    reset_context(search_path=(path,))
    context.validate_all()
except Exception as e:
    print(f"{type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
"""


def conda_free_env() -> dict[str, str]:
    """Environment with every CONDA*-prefixed variable stripped, so the
    oracle only ever sees the single fixture file passed via search_path."""
    return {k: v for k, v in os.environ.items() if not k.startswith("CONDA")}


def python_has_conda(python: str) -> bool:
    try:
        result = subprocess.run(
            [python, "-c", "import conda"],
            capture_output=True,
            env=conda_free_env(),
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    return result.returncode == 0


def find_conda_python() -> str:
    """Mirrors tests/condarc_conformance.rs's find_conda_python()."""
    env_python = os.environ.get("ALLEZ_CONFORMANCE_PYTHON")
    if env_python:
        if python_has_conda(env_python):
            return env_python
        sys.exit(
            f"$ALLEZ_CONFORMANCE_PYTHON={env_python!r} does not have `conda` importable"
        )

    if python_has_conda("python3"):
        return "python3"

    conda_path = shutil.which("conda")
    if conda_path:
        conda_dir = os.path.dirname(conda_path)
        for name in ("python3", "python"):
            candidate = os.path.join(conda_dir, name)
            if os.path.isfile(candidate) and python_has_conda(candidate):
                return candidate

    sys.exit(
        "no python interpreter with `conda` importable found (checked "
        "$ALLEZ_CONFORMANCE_PYTHON, `python3` on PATH, and the interpreter "
        "shipped alongside `conda` on PATH) -- this generator requires the "
        "same conda oracle tests/condarc_conformance.rs uses"
    )


def check_candidate(python: str, doc: dict) -> tuple[bool, str]:
    """Returns (is_valid, reason). reason is empty when valid."""
    text = json.dumps(doc)
    fd, path = tempfile.mkstemp(suffix=".yml")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        result = subprocess.run(
            [python, "-c", CONDA_CHECK_SCRIPT, path],
            capture_output=True,
            text=True,
            env=conda_free_env(),
            timeout=30,
        )
    finally:
        os.unlink(path)

    if result.returncode == 0:
        return True, ""
    return False, result.stderr.strip()


def clear_stale(directory: Path, prefix: str) -> None:
    stale = sorted(directory.glob(f"{prefix}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)")


def generate_accept_battery(python: str) -> tuple[list[str], list[tuple[str, str]]]:
    prefix = f"{KEY}_accept_"
    clear_stale(VALID_DIR, prefix)

    written: list[str] = []
    skipped: list[tuple[str, str]] = []
    for slug, value in ACCEPT_CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid:
            (VALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            skipped.append((filename, reason))
            print(f"SKIPPED {filename}  (value={value!r}): {reason}")
    return written, skipped


def generate_reject_battery(python: str) -> tuple[list[str], list[str]]:
    prefix = f"{KEY}_reject_"
    clear_stale(INVALID_DIR, prefix)

    written: list[str] = []
    unexpectedly_valid: list[str] = []
    for slug, value in REJECT_CANDIDATES:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if not is_valid:
            (INVALID_DIR / filename).write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r}): {reason[:100]}")
        else:
            unexpectedly_valid.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                "by conda -- this candidate belongs in the accept battery instead"
            )
    return written, unexpectedly_valid


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    print(f"--- {KEY}_accept_* ---\n")
    written_accept, skipped_accept = generate_accept_battery(python)
    print(
        f"\n{len(written_accept)} fixture(s) written, {len(skipped_accept)} "
        "candidate(s) skipped (accept).\n"
    )

    print(f"--- {KEY}_reject_* ---\n")
    written_reject, skipped_reject = generate_reject_battery(python)
    print(
        f"\n{len(written_reject)} fixture(s) written, {len(skipped_reject)} "
        "candidate(s) skipped (reject).\n"
    )

    total_written = len(written_accept) + len(written_reject)
    total_skipped = len(skipped_accept) + len(skipped_reject)
    print(f"=== total: {total_written} fixture(s) written, {total_skipped} skipped ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
