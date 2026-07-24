#!/usr/bin/env python3
"""Generate the exhaustive dict-of-lists-of-strings (`MapParameter
(SequenceParameter(str))`) `.condarc` key fixture battery:
`custom_multichannels_values_accept_*` / `custom_multichannels_values_reject_*`.

## Scope: which key this covers

Per `docs/condarc_research.md` §4 (and the follow-up container-type
survey done in chat, not yet appended to the doc as of this script),
exactly **one** `Context` parameter is declared as a `MapParameter`
whose *values* are themselves sequences of strings -- i.e. a JSON/YAML
object mapping multichannel names to lists of channel URLs/names:

    _custom_multichannels = ParameterLoader(
        MapParameter(SequenceParameter(PrimitiveParameter("", element_type=str))),
        aliases=("custom_multichannels",),
        expandvars=True,
    )

`custom_multichannels` (the alias, and the only realistic `.condarc`
spelling -- see `docs/condarc_research.md` §1.3 on underscored internal
names) is the only key with this container shape, so unlike the list-
of-strings and dict-of-strings batteries, this generator targets a
single key rather than a shared set. No custom `validation=` callable
touches it, no nullability is declared anywhere in its type, and it is
not one of the conda-build/CLI-only categories excluded from §4's
catalog -- fully in scope.

## Key empirically-confirmed findings backing the candidate lists below

This key is a genuine hybrid of the two previous batteries' findings --
`MapParameter`'s dict-of-strings rules govern the outer *keys* and the
raw *value* shape, and `SequenceParameter`'s list-of-strings rules
govern each value's *elements*. All confirmed against a real `conda`
installation (this script's own self-verification, same spirit as every
other generator in this directory):

1. **Outer shape matches `MapParameter`'s rules exactly, not
   `SequenceParameter`'s** (see `generate_dict_of_strings_condarc_
   fixtures.py`'s finding 1): the raw top-level value must be a genuine
   `Mapping` -- a bare list, even empty, is rejected outright
   (`InvalidTypeError`), in contrast to how a *value* one level down
   (finding 3 below) *does* get the Sequence-side "empty dict treated
   as empty list" leniency. A bare `null` for the whole key is treated
   identically to the key being absent (same `Parameter.get_all_
   matches`'s `m._raw_value is not None` filter documented for every
   other Map/Sequence-typed key in this suite).
2. **Keys (multichannel names) are never coerced, validated, or case-
   folded** -- identical to `generate_dict_of_strings_condarc_
   fixtures.py`'s finding 3: empty string, whitespace-only,
   leading/trailing-whitespace, arbitrary Unicode (CJK, emoji, RTL
   Arabic, combining diacritics, zero-width joiners), uppercase/mixed-
   case, numeric-looking, path-like/URL-like, dunder-prefixed, and
   newline/tab-embedded keys are all accepted verbatim.
3. **The same YAML 1.1 "simple key" 1024-character length ceiling
   applies to a multichannel name exactly as it did for a dict-of-
   strings key** (`generate_dict_of_strings_condarc_fixtures.py`'s
   finding 4) -- empirically re-bisected here and confirmed at the
   identical boundary: a key of raw length 1022 (1024 quoted
   characters) loads fine; length 1023 (1025 quoted characters) crashes
   with an uncaught `ParserError` from `ruamel.yaml`'s flow-mapping
   parser, before `Context` is even constructed. Included as a matched
   accept/reject pair pinned exactly at the boundary.
4. **A per-key *value* one level down gets `SequenceParameter`'s own
   leniency, not `MapParameter`'s** -- an empty dict `{}` assigned as a
   multichannel's value is silently treated as an empty list (`()`),
   exactly like the list-of-strings battery's `object_size_0_empty_
   treated_as_array` finding, because `SequenceParameter.load()`'s
   `isiterable(value)` check (not `isinstance(value, Mapping)`) is what
   governs a *value*'s shape here. A **non-empty** dict value, by
   contrast, crashes (`AttributeError: 'str' object has no attribute
   'value'`) trying to treat its string keys as raw sequence elements
   needing `.typify()` -- the same "emptiness, not shape" pattern
   documented repeatedly elsewhere in this suite. A bare scalar value
   (string/int/bool) is cleanly rejected (`InvalidTypeError`): per
   conda's own `isiterable()` predicate (not Python's native iterable
   protocol), a bare string does not count as iterable here, matching
   `docs/condarc_research.md` §2.4.
5. **A `null` *value* for one multichannel name resolves to an empty
   tuple for that name specifically, rather than omitting the name from
   the outer dict** -- distinct from finding 1's whole-key-`null`
   behavior. Concretely, `{"custom_multichannels": {"mc": null}}` loads
   successfully as `{"mc": ()}` (the key `"mc"` is present, mapped to
   an empty tuple), whereas `{"custom_multichannels": null}` makes the
   *entire* `custom_multichannels` key behave as if absent. Both are
   valid, but they are not the same "shape" of valid, and a
   hypothetical implementation could plausibly get one right and the
   other wrong.
6. **Each value-list's *elements* follow exactly the list-of-strings
   battery's element rules** (`generate_list_of_strings_condarc_
   fixtures.py`'s findings 1 and 5-6): non-string scalar elements
   (`int`/`float`/`bool`/`null`) all silently coerce via a bare
   `str(element)` call (`1 -> "1"`, `True -> "True"`, `None ->
   "None"`); a raw string element short-circuits `typify()` entirely
   (since the element type here is bare `str`, a single concrete
   class, exactly like the plain list-of-strings keys -- *not* like
   the nullable dict-of-strings keys' tuple type hint), so whitespace-
   only and otherwise-unusual string elements round-trip completely
   unchanged, with **no** stripping and **no** null-token special-
   casing. Arbitrary Unicode, control characters, NUL bytes, quotes/
   backslashes, YAML-syntax-lookalike content, and very long strings
   are all accepted verbatim as elements.
7. **A nested list/dict *element* (i.e. two containers deep -- a list,
   inside a multichannel's value list, inside the outer dict) crashes
   or cleanly rejects along exactly the same empty-vs-non-empty fault
   line documented throughout this suite** (`generate_list_of_strings_
   condarc_fixtures.py`'s finding 5): an empty nested container element
   fails cleanly (`InvalidTypeError`); a non-empty one crashes with an
   unhandled `AttributeError`. Both outcomes are still "invalid" (non-
   zero exit) either way.
8. **Duplicate elements within a single multichannel's value list are
   silently de-duplicated** -- this is *not* unique to nested
   sequences: `SequenceLoadedParameter.merge()` (`conda/common/
   configuration.py`) unconditionally runs every sequence's matches
   (even a single match from a single source) through `unique(...)`
   before returning the merged tuple. This applies equally to every
   *other* list-of-strings key in this suite (`channels`, `track_
   features`, etc.) -- it just never affected those batteries' pass/
   fail verdicts, since this conformance suite only asserts accept/
   reject, never the exact coerced value. Not modeled as a dedicated
   fixture here for the same reason: `["a", "a"]` is simply "valid",
   identically to `["a", "b"]`, regardless of what the deduplicated
   result looks like.

Every candidate (accept and reject) is verified empirically against a
real `conda` installation before a fixture is written -- an accept
candidate that's unexpectedly *rejected*, or a reject candidate that's
unexpectedly *accepted*, is reported as SKIPPED rather than silently
written to the wrong directory. Because this battery targets a single
key rather than a shared set applied across multiple keys, there is no
separate "exploded per-key" verification pass here (unlike the list-of-
strings/dict-of-strings generators) -- the whole-document check and the
single-key check are one and the same for this key.

Usage:
    python3 scripts/generate_dict_of_lists_condarc_fixtures.py

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

KEY = "custom_multichannels"

# ---------------------------------------------------------------------
# Candidate batteries -- (slug, value) pairs, each a candidate raw value
# for the single `custom_multichannels` key. See module docstring for
# the reasoning behind each group.
# ---------------------------------------------------------------------

ACCEPT: list[tuple[str, object]] = [
    ("dict_size_0_empty", {}),
    # Whole-key null == absent -- see finding 1.
    ("null_literal_treated_as_unset", None),
    # --- key-focused (multichannel name), size 1, fixed safe value ---
    ("dict_size_1_key_empty_string", {"": ["a"]}),
    ("dict_size_1_key_whitespace_only", {"   ": ["a"]}),
    ("dict_size_1_key_leading_trailing_whitespace", {"  key  ": ["a"]}),
    ("dict_size_1_key_unicode_cjk", {"日本語": ["a"]}),
    ("dict_size_1_key_unicode_emoji", {"😀🎉": ["a"]}),
    ("dict_size_1_key_unicode_rtl_arabic", {"مرحبا": ["a"]}),
    ("dict_size_1_key_unicode_combining_diacritic", {"e\u0301": ["a"]}),
    ("dict_size_1_key_unicode_zero_width_joiner", {"a\u200db": ["a"]}),
    ("dict_size_1_key_uppercase", {"CUSTOM": ["a"]}),
    ("dict_size_1_key_mixed_case", {"CuStOm": ["a"]}),
    ("dict_size_1_key_numeric_looking", {"123": ["a"]}),
    ("dict_size_1_key_path_like", {"pkgs/pro": ["a"]}),
    ("dict_size_1_key_url_like", {"http://example.com": ["a"]}),
    ("dict_size_1_key_dunder_prefixed", {"__cuda": ["a"]}),
    ("dict_size_1_key_with_newline", {"a\nb": ["a"]}),
    ("dict_size_1_key_with_tab", {"a\tb": ["a"]}),
    ("dict_size_1_key_moderately_long", {"k" * 200: ["a"]}),
    # Exactly at the YAML simple-key length boundary -- see finding 3.
    # Paired with `key_exceeds_yaml_simple_key_length_limit` in REJECT
    # (one character longer) to pin the precise cutoff.
    ("dict_size_1_key_yaml_simple_key_length_limit_boundary", {"k" * 1022: ["a"]}),
    ("dict_size_2_two_distinct_keys", {"a": ["x"], "b": ["y"]}),
    # --- value-shape-focused (fixed key "mc"), varying the value ---
    ("value_empty_list", {"mc": []}),
    ("value_size_2_list", {"mc": ["a", "b"]}),
    # null value -> empty tuple for that key specifically -- see
    # finding 5 (distinct from the whole-key-null case above).
    ("value_null_becomes_empty_tuple", {"mc": None}),
    # An empty dict value is silently treated as an empty list -- see
    # finding 4.
    ("value_empty_dict_treated_as_empty_list", {"mc": {}}),
    # --- element-content-focused (fixed key "mc", single-element list) ---
    ("element_empty_string", {"mc": [""]}),
    ("element_whitespace_preserved", {"mc": ["   "]}),
    ("element_whitespace_tabs_and_newlines_preserved", {"mc": ["\t\n"]}),
    ("element_unicode_cjk", {"mc": ["日本語"]}),
    ("element_unicode_emoji", {"mc": ["😀🎉"]}),
    ("element_unicode_rtl_arabic", {"mc": ["مرحبا"]}),
    ("element_unicode_combining_diacritic", {"mc": ["e\u0301"]}),
    ("element_unicode_zero_width_joiner", {"mc": ["a\u200db"]}),
    ("element_control_chars_tab_newline", {"mc": ["a\tb\nc"]}),
    ("element_null_byte", {"mc": ["a\0b"]}),
    ("element_quotes_and_backslash", {"mc": ["a\"b'c\\d"]}),
    ("element_yaml_lookalike_syntax", {"mc": ["- a: b"]}),
    ("element_numeric_looking_string", {"mc": ["123"]}),
    ("element_very_long_string", {"mc": ["a" * 10000]}),
    # Non-string scalar elements silently coerce via str() -- see
    # finding 6.
    ("element_int", {"mc": [1]}),
    ("element_negative_int", {"mc": [-5]}),
    ("element_float", {"mc": [1.5]}),
    ("element_bool_true", {"mc": [True]}),
    ("element_bool_false", {"mc": [False]}),
    ("element_null", {"mc": [None]}),
    # --- size 2 element lists ---
    ("element_size_2_distinct", {"mc": ["a", "b"]}),
    ("element_size_2_mixed_types", {"mc": ["a", 1]}),
    ("element_size_2_null_and_string", {"mc": [None, "a"]}),
    ("element_size_2_bool_and_string", {"mc": [True, "a"]}),
]

REJECT: list[tuple[str, object]] = [
    # Bare scalars for the whole key -- MapParameter requires a genuine
    # Mapping; a bare list (even empty) is rejected, unlike a
    # SequenceParameter's leniency toward a dict -- see finding 1.
    ("bare_string", "just-a-string"),
    ("bare_int", 5),
    ("bare_float", 5.5),
    ("bare_bool_true", True),
    ("bare_bool_false", False),
    ("bare_list_empty", []),
    ("bare_list_nonempty", ["a"]),
    # Bare scalar values one level down -- SequenceParameter's
    # isiterable() check rejects these too (a string doesn't count as
    # iterable per conda's own predicate) -- see finding 4.
    ("value_bare_string", {"mc": "foo"}),
    ("value_bare_int", {"mc": 5}),
    ("value_bare_bool", {"mc": True}),
    # A non-empty dict value crashes (paired with the accept battery's
    # `value_empty_dict_treated_as_empty_list`) -- see finding 4.
    ("value_nonempty_dict", {"mc": {"a": 1}}),
    # Nested-container elements -- see finding 7. Empty nested
    # containers fail cleanly; non-empty ones crash with an unhandled
    # AttributeError. Both are still "invalid" either way.
    ("element_nested_list_empty", {"mc": [[]]}),
    ("element_nested_list_nonempty", {"mc": [[1, 2]]}),
    ("element_nested_dict_empty", {"mc": [{}]}),
    ("element_nested_dict_nonempty", {"mc": [{"a": 1}]}),
    # One character past the YAML simple-key length boundary -- see
    # finding 3. Paired with the accept battery's `...boundary` fixture.
    ("key_exceeds_yaml_simple_key_length_limit", {"k" * 1023: ["a"]}),
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


def generate_battery(
    python: str,
    directory: Path,
    prefix: str,
    candidates: list[tuple[str, object]],
    expect_valid: bool,
) -> tuple[list[str], list[str]]:
    """Checks each (slug, value) candidate as the sole value of `KEY`
    against real conda, and writes a fixture only when the outcome
    matches `expect_valid`. Returns (written, unexpected)."""
    clear_stale(directory, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in candidates:
        doc = {KEY: value}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid == expect_valid:
            path = directory / filename
            path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            unexpected.append(filename)
            outcome = "ACCEPTED" if is_valid else "REJECTED"
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly {outcome} "
                f"by conda ({reason[:100]})"
            )
    return written, unexpected


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    total_written = 0
    total_unexpected = 0

    batteries = [
        (
            "--- custom_multichannels_values_accept_* ---",
            VALID_DIR,
            "custom_multichannels_values_accept_",
            ACCEPT,
            True,
        ),
        (
            "--- custom_multichannels_values_reject_* ---",
            INVALID_DIR,
            "custom_multichannels_values_reject_",
            REJECT,
            False,
        ),
    ]

    for label, directory, prefix, candidates, expect_valid in batteries:
        print(f"{label}\n")
        written, unexpected = generate_battery(python, directory, prefix, candidates, expect_valid)
        total_written += len(written)
        total_unexpected += len(unexpected)
        print(
            f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
            "skipped (unexpected outcome).\n"
        )

    print(
        f"=== total: {total_written} fixture(s) written, "
        f"{total_unexpected} skipped ==="
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
