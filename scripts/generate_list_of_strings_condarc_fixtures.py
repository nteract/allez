#!/usr/bin/env python3
"""Generate the exhaustive list-of-strings (`SequenceParameter(str)`)
`.condarc` key fixture battery: `list_of_strings_values_accept_*` /
`list_of_strings_values_reject_*` (valid/invalid for every plain
list-of-strings key, applied to all keys at once).

## Scope: which keys this covers

Per `docs/condarc_research.md` §4 (and the follow-up container-type
survey done in chat, not yet appended to the doc as of this script),
19 `Context` parameters are declared as a bare `SequenceParameter(str)`
-- i.e. a JSON/YAML list whose *elements* are plain strings, with no
extra tuple/nullable/enum shape and (with one exception, see below) no
custom `validation=` callable:

  `channels` (alias `channel`), `default_channels`,
  `allowlist_channels` (alias `whitelist_channels`),
  `denylist_channels`, `migrated_channel_aliases`, `repodata_fns`,
  `experimental`, `envs_dirs` (alias `envs_path`), `pkgs_dirs`,
  `preview`, `aggressive_update_packages`, `create_default_packages`,
  `disallowed_packages`, `pinned_packages`, `track_features`,
  `shortcuts_only`, `export_platforms` (alias `extra_platforms`),
  `subdirs`, `list_fields`.

**`list_fields` is deliberately excluded** from `ALL_LIST_OF_STR_KEYS`
below: per §3, it has a custom `list_fields_validation` callable that
restricts every element to the closed `CONDA_LIST_FIELDS` key set
(§5.9) -- applying an arbitrary-string battery to it would spuriously
reject valid-shape candidates (e.g. `["a", "b"]`) that are perfectly
fine for every *other* list-of-strings key. It needs its own,
narrower fixture battery (mirroring how `local_repodata_ttl` got its
own dedicated battery instead of joining the generic numeric one).
That leaves **18 keys** in `ALL_LIST_OF_STR_KEYS`.

Also out of scope, for reasons already recorded elsewhere:

  - `channel_settings` -- `SequenceParameter(MapParameter(str))`, a
    list of *maps*, not a list of strings. Different container shape
    entirely; not covered here.
  - conda-build variables and CLI-only variables -- out of scope for
    `conformance/condarc/**` entirely (`docs/condarc_research.md` §8
    item 5); §4's catalog already excludes both categories.

## Key empirically-confirmed findings backing the candidate lists below

All confirmed against a real `conda` installation (this script's own
self-verification, same spirit as every other generator in this
directory):

1. **Element-level coercion is a bare `str(element)` call, which
   essentially never raises.** Per `docs/condarc_research.md` §2.1's
   dispatch table, `typify_data_structure()` maps `typify(element,
   str)` over each raw list item once the raw *container* shape is
   confirmed to already be a list/iterable -- and for a non-tuple
   `element_type` that isn't `bool`, that's `str(element)`, a plain
   Python constructor call that succeeds for every JSON scalar: an
   `int`/`float`/`bool` element silently stringifies (`1 -> "1"`,
   `1.5 -> "1.5"`, `True -> "True"`), and a `None` element becomes the
   *string* `"None"` (not Python `None`, and not the same as the
   nullable-boolish `NULL_STRINGS` behavior elsewhere in this codebase
   -- there's no nullability concept for a plain `str`-typed sequence
   element at all). A raw `str` element already *is* a `str`, so
   `typify_data_structure`'s `isinstance(value, str) and
   issubclass(element_type, str)` special case skips `typify()`
   entirely and preserves it byte-for-byte, whitespace and all (same
   short-circuit §2.1 documents for scalar `str`-typed keys).
2. **A raw JSON `null` for the *whole key* (not an element) is treated
   identically to the key being entirely absent -- it is *not*
   rejected, and does *not* even produce an empty list; it falls back
   to that key's own default** (empirically confirmed: `{"track_features":
   null}` and `{}` both resolve `context.track_features == ()`, and
   `{"channels": null}` resolves the same as omitting `channels`
   entirely). This is a real behavioral quirk worth its own dedicated
   accept fixture (`null_literal_treated_as_unset`) -- a hypothetical
   implementation that instead raises on `null` for a non-nullable
   sequence key would disagree with real conda here.
3. **A bare, raw *empty* JSON object (`{}`) is silently treated as an
   empty list, not rejected** -- `SequenceParameter.load()`'s
   `isiterable(value)` check (§2.4) is satisfied by a `dict` too (a
   `dict` is iterable -- over its *keys*), and an empty dict has zero
   keys to iterate/typify, so it resolves to `()` exactly like `[]`.
   **A *non-empty* dict, by contrast, crashes** (see finding 5 below)
   -- this is the "emptiness, not shape" pattern already documented in
   `docs/condarc_research.md` §8 item 7 for scalar-typed keys, now
   confirmed to recur identically for sequence-typed ones, and is
   deliberately included as `object_size_0_empty_treated_as_array` in
   the accept battery (paired with `object_nonempty` in the reject
   battery to make the empty/non-empty contrast explicit).
4. **A bare scalar (string/int/float/bool) for the whole key is
   cleanly rejected with `InvalidTypeError`**, per §2.4: sequence-typed
   keys require a real YAML/JSON list (or dict, per finding 3) at the
   raw level -- a bare scalar is never auto-wrapped into a
   single-element list.
5. **A list containing a nested, non-empty list or dict element
   crashes with an unhandled `AttributeError`** (`'YamlRawParameter'
   object has no attribute 'typify'`), the same *shape* of bug already
   documented in `docs/condarc_research.md` §8 item 7 for boolish/
   numeric keys, now confirmed for list-of-strings keys too. A nested
   *empty* list/dict element does **not** crash -- it typifies cleanly
   to an empty `tuple`/`frozendict`, which then fails the *outer*
   list's per-element `isinstance(_, str)` check with a clean
   `InvalidTypeError` instead (mirroring the same "emptiness, not
   shape" distinction as finding 3, one level down). Both outcomes are
   still "invalid" (non-zero exit) for this suite's purposes, so both
   the empty and non-empty nested shapes are included in the reject
   battery to document the actual crash-vs-clean-rejection boundary --
   worth remembering if a future contributor asserts on conda's exact
   error *text* rather than just pass/fail.
6. Ordinary string content -- **empty strings, whitespace-only strings
   (including tabs/newlines), arbitrary Unicode (CJK, emoji, accented
   Latin, RTL Arabic, combining diacritics, zero-width joiners),
   embedded control characters, NUL bytes, quotes/backslashes, and
   YAML-syntax-lookalike text (`"- a: b"`, `"# comment"`)** -- all pass
   through completely unvalidated for every list-of-strings key: none
   of the 18 keys here declare a custom `validation=` callable (§3),
   so there is no length limit, no character-set restriction, and no
   "must look like a path/URL/etc." check at parse time, unlike (say)
   `channel_alias`'s scheme requirement or `default_python`'s version-
   range check.
7. **Duplicate elements are preserved at the raw-parameter level** --
   deduplication (e.g. `pkgs_dirs`'s `dict.fromkeys(...)`,
   `channels`'s multichannel-expansion-and-dedup logic) is a read-side
   `@property` transform on a *few* of these keys, not a parse-time
   validation rule, so `["a", "a"]` is unconditionally valid (loads
   without error) for every key in this battery regardless of whether
   that key happens to have a dedup-on-read property.

Every candidate (both accept and reject) is verified empirically
against a real `conda` installation before a fixture is written, for
the same self-correcting reason as every other generator in this
directory -- an accept candidate that's unexpectedly *rejected*, or a
reject candidate that's unexpectedly *accepted* for even one of the 18
keys, is reported as SKIPPED rather than silently written to the wrong
directory. Reject candidates are additionally verified **exploded
per-key** (not just as one shared 18-key document), matching how
`tests/condarc_conformance.rs`'s `invalid_condarc_is_rejected` actually
exercises `invalid/` fixtures at test time (see that file's module
docs) -- a candidate that fails for the shared document only because
*some* key chokes on it first would otherwise slip through generation-
time verification undetected.

Usage:
    python3 scripts/generate_list_of_strings_condarc_fixtures.py

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

# ---------------------------------------------------------------------
# Key catalog -- see module docstring's "Scope" section. `list_fields`
# is deliberately excluded (custom validation; needs its own battery).
# ---------------------------------------------------------------------
ALL_LIST_OF_STR_KEYS: list[str] = [
    "channels",
    "default_channels",
    "allowlist_channels",
    "denylist_channels",
    "migrated_channel_aliases",
    "repodata_fns",
    "experimental",
    "envs_dirs",
    "pkgs_dirs",
    "preview",
    "aggressive_update_packages",
    "create_default_packages",
    "disallowed_packages",
    "pinned_packages",
    "track_features",
    "shortcuts_only",
    "export_platforms",
    "subdirs",
]

# ---------------------------------------------------------------------
# Candidate batteries -- (slug, value) pairs. See module docstring for
# the reasoning behind each group. Sizes 0/1/2 are covered explicitly
# per-slug, per the motivating ask (exhaustive edge cases at each of
# those three sizes: empty/whitespace/unicode content, wrong element
# types that still coerce, etc).
# ---------------------------------------------------------------------

SHARED_ACCEPT: list[tuple[str, object]] = [
    # --- size 0 ---
    ("array_size_0_empty", []),
    # A bare empty *object* is also silently treated as an empty list
    # (finding 3) -- deliberately paired with `object_nonempty` in
    # SHARED_REJECT to make the empty/non-empty contrast explicit.
    ("object_size_0_empty_treated_as_array", {}),
    # --- size 1 ---
    ("array_size_1_empty_string", [""]),
    ("array_size_1_whitespace_only", ["   "]),
    ("array_size_1_whitespace_tabs_and_newlines", ["\t\n"]),
    ("array_size_1_unicode_cjk", ["日本語"]),
    ("array_size_1_unicode_emoji", ["😀🎉"]),
    ("array_size_1_unicode_accented_latin", ["café"]),
    ("array_size_1_unicode_rtl_arabic", ["مرحبا"]),
    # "e" + U+0301 COMBINING ACUTE ACCENT, two separate codepoints.
    ("array_size_1_unicode_combining_diacritic", ["e\u0301"]),
    ("array_size_1_unicode_zero_width_joiner", ["a\u200db"]),
    ("array_size_1_control_chars_tab_newline", ["a\tb\nc"]),
    ("array_size_1_null_byte", ["a\0b"]),
    ("array_size_1_quotes_and_backslash", ["a\"b'c\\d"]),
    # Looks like YAML list/mapping/comment syntax, but it's the
    # *content* of a JSON string element, not raw YAML -- must be
    # accepted literally, not parsed as nested structure.
    ("array_size_1_yaml_lookalike_syntax", ["- a: b"]),
    ("array_size_1_numeric_looking_string", ["123"]),
    ("array_size_1_very_long_string", ["a" * 10000]),
    # Non-string JSON element types all silently coerce via str() --
    # see finding 1.
    ("array_size_1_element_int", [1]),
    ("array_size_1_element_float", [1.5]),
    ("array_size_1_element_negative_int", [-5]),
    ("array_size_1_element_bool_true", [True]),
    ("array_size_1_element_bool_false", [False]),
    ("array_size_1_element_null", [None]),
    # --- size 2 ---
    ("array_size_2_distinct_strings", ["a", "b"]),
    # Duplicates are preserved at the raw-parameter level -- see
    # finding 7. (Any read-side dedup is a `@property` transform on a
    # few specific keys, not a parse-time rule shared by all of them.)
    ("array_size_2_duplicate_strings", ["a", "a"]),
    ("array_size_2_empty_and_nonempty", ["", "a"]),
    ("array_size_2_unicode_and_ascii", ["café", "plain"]),
    ("array_size_2_mixed_element_types", ["a", 1]),
    ("array_size_2_null_and_string", [None, "a"]),
    ("array_size_2_bool_and_string", [True, "a"]),
    # --- whole-key null (distinct from an array *containing* null) ---
    # A raw `null` for the whole key is treated as if the key were
    # entirely absent, falling back to that key's own default -- see
    # finding 2. Not merely "valid": genuinely indistinguishable from
    # omission, which a naive nullable-sequence implementation might
    # not replicate.
    ("null_literal_treated_as_unset", None),
]

# Invalid for every key in ALL_LIST_OF_STR_KEYS.
SHARED_REJECT: list[tuple[str, object]] = [
    # Bare scalars for the whole key -- sequence-typed keys require a
    # real list (or, per finding 3, an empty dict) at the raw level; a
    # bare scalar is never auto-wrapped into a single-element list
    # (finding 4).
    ("bare_string", "just-a-string"),
    ("bare_int", 5),
    ("bare_float", 5.5),
    ("bare_bool_true", True),
    ("bare_bool_false", False),
    # Paired with `object_size_0_empty_treated_as_array` in
    # SHARED_ACCEPT: only the *empty* dict is silently valid (finding
    # 3); a non-empty one crashes trying to treat its string keys as
    # raw list elements needing `.typify()`.
    ("object_nonempty", {"a": 1}),
    # Nested-container elements -- see finding 5. Empty nested
    # containers fail *cleanly*; non-empty ones crash with an unhandled
    # AttributeError. Both are still "invalid" (non-zero exit) either
    # way, so both shapes are included here to document the actual
    # crash-vs-clean-rejection boundary.
    ("array_nested_empty_array", [[]]),
    ("array_nested_nonempty_array", [[1, 2]]),
    ("array_nested_empty_object", [{}]),
    ("array_nested_nonempty_object", [{"a": 1}]),
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


def generate_accept_battery(
    python: str,
    directory: Path,
    prefix: str,
    keys: list[str],
    candidates: list[tuple[str, object]],
) -> tuple[list[str], list[str]]:
    """Applies each (slug, value) candidate to every key in `keys` at
    once (one shared multi-key document per candidate), checks the
    *whole document* against real conda, and writes a fixture only
    when it's accepted. Returns (written, unexpected)."""
    clear_stale(directory, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in candidates:
        doc = {key: value for key in keys}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid:
            path = directory / filename
            path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            unexpected.append(filename)
            print(
                f"SKIPPED {filename}  (value={value!r}): unexpectedly REJECTED "
                f"by conda ({reason[:100]})"
            )
    return written, unexpected


def generate_reject_battery(
    python: str,
    directory: Path,
    prefix: str,
    keys: list[str],
    candidates: list[tuple[str, object]],
) -> tuple[list[str], list[str]]:
    """Like `generate_accept_battery`, but additionally verifies each
    candidate *exploded* per-key (one single-key document per key in
    `keys`), matching how `tests/condarc_conformance.rs`'s
    `invalid_condarc_is_rejected` actually exercises `invalid/`
    fixtures at test time (see that file's module docs and this
    script's module docstring). A candidate is only written if *both*
    the shared whole-document check *and* every individual per-key
    check reject it."""
    clear_stale(directory, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in candidates:
        doc = {key: value for key in keys}
        whole_valid, whole_reason = check_candidate(python, doc)

        bad_keys: list[str] = []
        per_key_reason = ""
        for key in keys:
            key_valid, key_reason = check_candidate(python, {key: value})
            if key_valid:
                bad_keys.append(key)
                per_key_reason = key_reason

        filename = f"{prefix}{slug}.json"
        if not whole_valid and not bad_keys:
            path = directory / filename
            path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            unexpected.append(filename)
            if bad_keys:
                print(
                    f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                    f"by conda for key(s) {bad_keys} ({per_key_reason[:100]})"
                )
            else:
                print(
                    f"SKIPPED {filename}  (value={value!r}): unexpectedly ACCEPTED "
                    f"by conda as whole document ({whole_reason[:100]})"
                )
    return written, unexpected


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)
    INVALID_DIR.mkdir(parents=True, exist_ok=True)

    total_written = 0
    total_unexpected = 0

    print(f"--- list_of_strings_values_accept_* ({len(ALL_LIST_OF_STR_KEYS)} keys) ---\n")
    written, unexpected = generate_accept_battery(
        python,
        VALID_DIR,
        "list_of_strings_values_accept_",
        ALL_LIST_OF_STR_KEYS,
        SHARED_ACCEPT,
    )
    total_written += len(written)
    total_unexpected += len(unexpected)
    print(
        f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
        "skipped (unexpected outcome).\n"
    )

    print(f"--- list_of_strings_values_reject_* ({len(ALL_LIST_OF_STR_KEYS)} keys) ---\n")
    written, unexpected = generate_reject_battery(
        python,
        INVALID_DIR,
        "list_of_strings_values_reject_",
        ALL_LIST_OF_STR_KEYS,
        SHARED_REJECT,
    )
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
