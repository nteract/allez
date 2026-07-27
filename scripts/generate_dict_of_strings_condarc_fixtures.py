#!/usr/bin/env python3
"""Generate the exhaustive dict-of-strings (`MapParameter(str)` /
`MapParameter((str, NoneType))`) `.condarc` key fixture battery:
`dict_of_strings_values_accept_*` / `dict_of_strings_values_accept_non_nullable_*`
/ `dict_of_strings_values_accept_nullable_*` / `dict_of_strings_values_reject_*`.

## Scope: which keys this covers

Per `docs/condarc_research.md` §4 (and the follow-up container-type survey
done in chat, not yet appended to the doc as of this script), exactly 4
canonical `Context` parameters are declared as a `MapParameter` whose
*values* are plain strings (possibly nullable) -- i.e. a JSON/YAML object
mapping string keys to string (or null) values:

  - **`NON_NULLABLE_KEYS`** (2, `MapParameter(PrimitiveParameter("",
    element_type=str))` -- values are plain `str`, **not** nullable):
    `custom_channels`, `migrated_custom_channels`.
  - **`NULLABLE_KEYS`** (2, `MapParameter(PrimitiveParameter(None,
    element_type=(str, NoneType)))` -- values may be `null`):
    `override_virtual_packages`, `proxy_servers`.

`override_virtual_packages`'s alias `virtual_packages` is deliberately
**not** re-tested here: alias-spelling coverage already exists in
`aliases_accept_alias_spellings_all_params.json` (`scripts/
generate_alias_multiplekeys_condarc_fixtures.py`), so re-including it in
this generic-value battery would just be redundant with that dedicated
alias battery.

Out of scope, for reasons already recorded elsewhere:

  - `custom_multichannels` -- `MapParameter(SequenceParameter(str))`, a
    dict of *lists*, not a dict of strings. Different container shape;
    not covered here.
  - conda-build variables and CLI-only variables -- out of scope for
    `conformance/condarc/**` entirely (`docs/condarc_research.md` §8
    item 5); §4's catalog already excludes both categories.

## Key empirically-confirmed findings backing the candidate lists below

All confirmed against a real `conda` installation (this script's own
self-verification, same spirit as every other generator in this
directory). The single most important thing this battery exists to
capture: **nullable and non-nullable dict-value keys behave differently
for the exact same raw value**, for a subtle type-coercion reason (finding
5 below) -- hence the explicit 3-way split into `SHARED_ACCEPT` /
`NON_NULLABLE_ONLY_ACCEPT` / `NULLABLE_ONLY_ACCEPT` instead of one
undifferentiated battery.

1. **`MapParameter.load()` requires a genuine `Mapping` at the raw
   level -- unlike `SequenceParameter` (see `generate_list_of_strings_
   condarc_fixtures.py`'s finding 3), a `dict`-typed key does *not*
   accept a bare list, not even an empty one.** (`conda/common/
   configuration.py`'s `MapParameter.load` checks `isinstance(value,
   Mapping)`, not `isiterable(value)` -- the latter is what lets
   `SequenceParameter` silently accept a dict-shaped raw value.)
   Empirically confirmed: `{"custom_channels": []}` is cleanly rejected
   with `InvalidTypeError` for every one of the 4 keys here, in sharp
   asymmetry with the list-of-strings battery's `object_size_0_empty_
   treated_as_array` finding.
2. **A raw JSON `null` for the *whole key* is treated identically to
   the key being entirely absent** -- exactly the same "filtered out of
   `get_all_matches` before it ever reaches `.load()`" mechanism
   documented for `SequenceParameter` (`docs/condarc_research.md`-
   adjacent finding, `Parameter.get_all_matches`'s `m._raw_value is not
   None` filter is shared by both `MapParameter` and `SequenceParameter`).
   Not merely "valid": genuinely indistinguishable from omission.
3. **Dict *keys* are never coerced, validated, or case-folded in any
   way** -- they come straight from the raw YAML/JSON mapping's string
   keys and are used as literal dict keys throughout. There is no
   length limit, capitalization rule, or character-set restriction
   at the `Context`/`Configuration` level for any of these 4 keys (no
   custom `validation=` callable touches keys specifically; `ssl_verify`/
   `channel_alias`/`default_python`'s validation callables are unrelated
   scalar parameters). Confirmed accepted verbatim: empty string keys,
   whitespace-only keys, leading/trailing-whitespace keys, arbitrary
   Unicode (CJK, emoji, RTL Arabic, combining diacritics, zero-width
   joiners), uppercase/mixed-case keys, numeric-looking keys, path-like
   and URL-like keys, a dunder-prefixed key (`__cuda` -- notable because
   `override_virtual_packages`'s *read-side* `@property` strips a
   leading `__` for lookup purposes, per `docs/condarc_research.md` §4.7's
   note on that key, but that's a read-side transform, not a parse-time
   requirement -- the raw dunder-prefixed key loads exactly as given),
   and keys containing embedded newlines/tabs.
4. **There is, however, a real length ceiling on a dict key -- not a
   conda-specific rule, but the YAML 1.1 "simple key" 1024-character
   restriction enforced by `ruamel.yaml`'s flow-mapping parser, which
   every fixture in this suite goes through** (fixtures are always
   JSON-serialized text, which is valid YAML flow-mapping syntax; see
   `docs/condarc_research.md`'s `numeric`/`boolish` generators' own
   notes on JSON-only fixtures still being *read* as YAML). Empirically
   bisected: a quoted-string dict key of raw length 1022 (i.e. 1024
   characters once its JSON/YAML quote marks are counted) loads fine;
   raw length 1023 (1025 quoted characters) raises an uncaught
   `ParserError: while parsing a flow mapping ... expected ',' or '}',
   but got ':'` for *every* one of the 4 keys here, both as a shared
   multi-key document and exploded to a single-key document -- this is
   a whole-*document* YAML parse failure (it happens before `Context`
   is even constructed), not a per-parameter validation error, so it's
   included as a `reject` fixture (`key_exceeds_yaml_simple_key_length_
   limit`) alongside a boundary-safe `accept` counterpart pinned exactly
   at the 1022-character edge (`key_yaml_simple_key_length_limit_
   boundary`) to document the precise cutoff. A more modest "just a
   fairly long key" case (200 characters, nowhere near the limit) is
   included separately so the boundary case doesn't have to double as
   the only "long key" coverage.
5. **The whitespace-preservation short-circuit that keeps a plain
   `str`-typed value byte-for-byte (`docs/condarc_research.md` §2.1's
   note on `typify_data_structure` skipping `typify()` for `isinstance
   (value, str) and issubclass(type_hint, str)`) only fires when
   `element_type` is a single concrete class -- it does *not* fire when
   `element_type` is a tuple like `(str, NoneType)`, because `issubclass
   (a_tuple, str)` isn't the check being performed (the code explicitly
   requires `isinstance(type_hint, type)` first).** Concretely, walking
   `MapLoadedParameter.typify()` -> `LoadedParameter._typify_data_
   structure` for each dict *value* (a nested `PrimitiveLoadedParameter`,
   itself calling `typify()` on its own scalar value):
     - **Non-nullable keys** (`element_type=str`, a single class): the
       short-circuit fires. A whitespace-only string value round-trips
       **completely unchanged** (`"   " -> "   "`), and the literal
       strings `"none"`/`"None"`/`"NONE"` stay **literal strings** --
       there is no `(str, NoneType)` branch to special-case them,
       because the type hint is bare `str`.
     - **Nullable keys** (`element_type=(str, NoneType)`, a tuple): the
       short-circuit's `isinstance(type_hint, type)` check fails (a
       tuple is not a `type`), so it falls through to the real
       `typify(value, type_hint)` call -- which (a) unconditionally
       `.strip()`s any string value first (`typify()`'s very first
       lines, per `docs/condarc_research.md` §2.1's note on the
       no-hint path -- turns out to apply here too, for a *tuple*
       hint, not just the no-hint path), so a whitespace-only string
       value collapses to `""`; and (b) then applies the `{str,
       NoneType}`-tuple dispatch rule (§2.1): `str(value)`, **except**
       the literal string `"none"` (case-insensitively -- confirmed for
       `"none"`/`"None"`/`"NONE"`/`"NoNe"`) becomes Python `None`. Every
       *other* would-be-null-ish token elsewhere in this codebase's
       boolish `NULL_STRINGS` tuple (`"~"`, `"null"`, `"\0"`) is
       **not** special-cased here and stays a literal string -- this
       nullable-dict-value null-token vocabulary is narrower than, and
       unrelated to, `boolify()`'s own `NULL_STRINGS`/`BOOLISH_FALSE`
       tuples documented for the scalar boolish keys.
   This is precisely why `NON_NULLABLE_ONLY_ACCEPT` and `NULLABLE_ONLY_
   ACCEPT` exist as separate batteries applied to disjoint key sets,
   rather than one shared battery applied to all 4: the exact same raw
   value (a whitespace-only string, or the string `"none"`) produces a
   *different, mutually exclusive* outcome depending on nullability.
6. **Non-string scalar dict values (`int`/`float`/`bool`/`null`) all
   silently coerce via a bare `str(value)` call, identically to list
   elements** (see `generate_list_of_strings_condarc_fixtures.py`'s
   finding 1) **for non-nullable keys** -- `5 -> "5"`, `True -> "True"`,
   and (notably) a bare `null` *value* (as opposed to a `null` for the
   *whole key*, finding 2) becomes the literal string `"None"`, not
   Python `None`, because there's no nullable branch to catch it. For
   **nullable** keys, the same non-string scalars coerce the same way
   *except* `null`, which stays Python `None` as expected (nullable
   type hint's whole point).
7. **A nested list/dict *value* crashes or cleanly rejects along
   exactly the same empty-vs-non-empty fault line already documented
   for list-of-strings elements** (`generate_list_of_strings_condarc_
   fixtures.py`'s finding 5) -- an empty nested container value fails
   cleanly (`InvalidTypeError`); a non-empty one crashes with an
   unhandled `AttributeError`. Both outcomes are still "invalid"
   (non-zero exit) either way, and this behavior is identical for
   nullable and non-nullable keys alike (neither `{str}` nor `{str,
   NoneType}` includes a container type), so these live in the single
   shared `SHARED_REJECT` battery rather than being split by nullability.

Every candidate (accept and reject, all three/four batteries) is
verified empirically against a real `conda` installation before a
fixture is written. Reject candidates are additionally verified
**exploded per-key** (not just as one shared multi-key document),
matching how `tests/condarc_conformance.rs`'s `invalid_condarc_is_
rejected` actually exercises `invalid/` fixtures at test time -- see
`generate_list_of_strings_condarc_fixtures.py`'s module docstring for
why this matters.

Usage:
    python3 scripts/generate_dict_of_strings_condarc_fixtures.py

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
# Key catalog -- see module docstring's "Scope" section.
# ---------------------------------------------------------------------
NON_NULLABLE_KEYS: list[str] = [
    "custom_channels",
    "migrated_custom_channels",
]
NULLABLE_KEYS: list[str] = [
    "override_virtual_packages",
    "proxy_servers",
]
ALL_DICT_OF_STR_KEYS: list[str] = NON_NULLABLE_KEYS + NULLABLE_KEYS

# ---------------------------------------------------------------------
# Candidate batteries -- (slug, value) pairs. See module docstring for
# the reasoning behind each group.
# ---------------------------------------------------------------------

# Valid for all 4 keys regardless of value-nullability -- these vary the
# *key* (with a safe constant "val" value) or use value content/types
# that coerce identically either way (see finding 6).
SHARED_ACCEPT: list[tuple[str, object]] = [
    ("dict_size_0_empty", {}),
    # Whole-key null == absent -- see finding 2.
    ("null_literal_treated_as_unset", None),
    # --- key-focused (size 1, fixed safe value) ---
    ("dict_size_1_key_empty_string", {"": "val"}),
    ("dict_size_1_key_whitespace_only", {"   ": "val"}),
    ("dict_size_1_key_leading_trailing_whitespace", {"  key  ": "val"}),
    ("dict_size_1_key_unicode_cjk", {"日本語": "val"}),
    ("dict_size_1_key_unicode_emoji", {"😀🎉": "val"}),
    ("dict_size_1_key_unicode_rtl_arabic", {"مرحبا": "val"}),
    ("dict_size_1_key_unicode_combining_diacritic", {"e\u0301": "val"}),
    ("dict_size_1_key_unicode_zero_width_joiner", {"a\u200db": "val"}),
    ("dict_size_1_key_uppercase", {"HTTP": "val"}),
    ("dict_size_1_key_mixed_case", {"HttP": "val"}),
    ("dict_size_1_key_numeric_looking", {"123": "val"}),
    ("dict_size_1_key_path_like", {"pkgs/pro": "val"}),
    ("dict_size_1_key_url_like", {"http://example.com": "val"}),
    # Read-side __-stripping is a lookup-time transform, not a parse
    # requirement -- see finding 3.
    ("dict_size_1_key_dunder_prefixed", {"__cuda": "val"}),
    ("dict_size_1_key_with_newline", {"a\nb": "val"}),
    ("dict_size_1_key_with_tab", {"a\tb": "val"}),
    ("dict_size_1_key_moderately_long", {"k" * 200: "val"}),
    # Exactly at the YAML simple-key length boundary -- see finding 4.
    # Paired with `key_exceeds_yaml_simple_key_length_limit` in
    # SHARED_REJECT (one character longer) to pin the precise cutoff.
    ("dict_size_1_key_yaml_simple_key_length_limit_boundary", {"k" * 1022: "val"}),
    # --- size 2 ---
    ("dict_size_2_two_distinct_keys", {"a": "val", "b": "val"}),
    # --- value-focused (size 1, fixed safe key), shared across
    # nullability because these particular values coerce identically
    # either way (see finding 6) ---
    ("dict_size_1_value_plain_string", {"k": "plain"}),
    ("dict_size_1_value_empty_string", {"k": ""}),
    ("dict_size_1_value_unicode", {"k": "café😀日本語"}),
    ("dict_size_1_value_numeric_looking_string", {"k": "123"}),
    ("dict_size_1_value_element_int", {"k": 5}),
    ("dict_size_1_value_element_negative_int", {"k": -5}),
    ("dict_size_1_value_element_float", {"k": 1.5}),
    ("dict_size_1_value_element_bool_true", {"k": True}),
    ("dict_size_1_value_element_bool_false", {"k": False}),
]

# Valid ONLY for NON_NULLABLE_KEYS -- see finding 5. Whitespace is
# preserved verbatim (no strip), and "none"-ish tokens stay literal
# strings, because element_type is bare `str`, not a tuple.
NON_NULLABLE_ONLY_ACCEPT: list[tuple[str, object]] = [
    ("dict_size_1_value_whitespace_preserved", {"k": "   "}),
    ("dict_size_1_value_whitespace_tabs_and_newlines_preserved", {"k": "\t\n"}),
    ("dict_size_1_value_none_string_lower_stays_literal", {"k": "none"}),
    ("dict_size_1_value_none_string_upper_stays_literal", {"k": "NONE"}),
    # A bare null *value* (not a whole-key null, finding 2) becomes the
    # *string* "None" -- there's no nullable branch to catch it here.
    ("dict_size_1_value_element_null_becomes_string_none", {"k": None}),
]

# Valid ONLY for NULLABLE_KEYS -- see finding 5. Whitespace is stripped
# (collapses to ""), and only the exact token "none" (any case)
# resolves to Python None; every other null-ish token elsewhere in this
# codebase ("~", "null", "\0", "non") stays a literal string here.
NULLABLE_ONLY_ACCEPT: list[tuple[str, object]] = [
    ("dict_size_1_value_element_null", {"k": None}),
    ("dict_size_1_value_none_string_lower_becomes_null", {"k": "none"}),
    ("dict_size_1_value_none_string_upper_becomes_null", {"k": "NONE"}),
    ("dict_size_1_value_none_string_mixedcase_becomes_null", {"k": "NoNe"}),
    ("dict_size_1_value_whitespace_stripped_to_empty", {"k": "   "}),
    ("dict_size_1_value_whitespace_tabs_and_newlines_stripped", {"k": "\t\n"}),
    ("dict_size_1_value_non_token_stays_literal", {"k": "non"}),
    ("dict_size_1_value_tilde_stays_literal", {"k": "~"}),
    ("dict_size_1_value_null_word_stays_literal", {"k": "null"}),
]

# Invalid for every key in ALL_DICT_OF_STR_KEYS, regardless of
# nullability (see finding 7 for why nullability doesn't matter here).
SHARED_REJECT: list[tuple[str, object]] = [
    # Bare scalars for the whole key.
    ("bare_string", "just-a-string"),
    ("bare_int", 5),
    ("bare_float", 5.5),
    ("bare_bool_true", True),
    ("bare_bool_false", False),
    # Unlike SequenceParameter (which silently treats a dict as an
    # iterable, see the list-of-strings battery's finding 3),
    # MapParameter strictly requires a Mapping -- a bare list, even
    # empty, is rejected outright. See finding 1.
    ("bare_list_empty", []),
    ("bare_list_nonempty", ["a"]),
    # Nested-container values -- see finding 7. Empty nested containers
    # fail cleanly; non-empty ones crash with an unhandled
    # AttributeError. Both are still "invalid" either way.
    ("value_nested_list_empty", {"k": []}),
    ("value_nested_list_nonempty", {"k": [1, 2]}),
    ("value_nested_dict_empty", {"k": {}}),
    ("value_nested_dict_nonempty", {"k": {"a": 1}}),
    # One character past the YAML simple-key length boundary -- see
    # finding 4. Paired with the accept battery's `...boundary` fixture.
    #
    # Also has a crate-only mirror-image `accept_key_exceeds_yaml_simple_key_length_limit`
    # fixture in valid/ (same 1023-character key): `yaml-rust2` (the condarc crate's YAML
    # dependency) enforces no equivalent "simple key" length limit at all, so the crate accepts
    # what real conda rejects here -- this fixture itself is named in
    # tests/condarc_conformance.rs's `CRATE_SKIPPED_FIXTURES` (the `Crate` checker is skipped
    # entirely for it, never invoked) -- see docs/condarc_research.md item 21,
    # tests/condarc_conformance.rs's `RUST_ONLY_FIXTURES`, and this repo's
    # generate_zzz_condarc_expected_fixtures.py's `CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES` (that
    # mirror fixture's `expected/*.json` is hand-authored, not conda-oracle-generated, since
    # real conda has no live value to record for it).
    ("key_exceeds_yaml_simple_key_length_limit", {"k" * 1023: "val"}),
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
    fixtures at test time. A candidate is only written if *both* the
    shared whole-document check *and* every individual per-key check
    reject it."""
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

    batteries = [
        (
            f"--- dict_of_strings_values_accept_* ({len(ALL_DICT_OF_STR_KEYS)} keys) ---",
            VALID_DIR,
            "dict_of_strings_values_accept_",
            ALL_DICT_OF_STR_KEYS,
            SHARED_ACCEPT,
        ),
        (
            f"--- dict_of_strings_values_accept_non_nullable_* ({len(NON_NULLABLE_KEYS)} keys) ---",
            VALID_DIR,
            "dict_of_strings_values_accept_non_nullable_",
            NON_NULLABLE_KEYS,
            NON_NULLABLE_ONLY_ACCEPT,
        ),
        (
            f"--- dict_of_strings_values_accept_nullable_* ({len(NULLABLE_KEYS)} keys) ---",
            VALID_DIR,
            "dict_of_strings_values_accept_nullable_",
            NULLABLE_KEYS,
            NULLABLE_ONLY_ACCEPT,
        ),
    ]

    for label, directory, prefix, keys, candidates in batteries:
        print(f"{label}\n")
        written, unexpected = generate_accept_battery(python, directory, prefix, keys, candidates)
        total_written += len(written)
        total_unexpected += len(unexpected)
        print(
            f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
            "skipped (unexpected outcome).\n"
        )

    print(f"--- dict_of_strings_values_reject_* ({len(ALL_DICT_OF_STR_KEYS)} keys) ---\n")
    written, unexpected = generate_reject_battery(
        python,
        INVALID_DIR,
        "dict_of_strings_values_reject_",
        ALL_DICT_OF_STR_KEYS,
        SHARED_REJECT,
    )
    total_written += len(written)
    total_unexpected += len(unexpected)
    print(
        f"\n{len(written)} fixture(s) written, {len(unexpected)} candidate(s) "
        "skipped (unexpected outcome).\n"
    )

    # `key_exceeds_yaml_simple_key_length_limit` above (in `SHARED_REJECT`) documents that real
    # conda rejects a 1023-character raw key outright, one character past `ruamel.yaml`'s
    # 1024-character "simple key" scanner limit. This crate's own YAML dependency (`yaml-rust2`)
    # enforces no equivalent limit at all -- see docs/condarc_research.md item 21 -- so a
    # mirror-image `dict_of_strings_values_accept_key_exceeds_yaml_simple_key_length_limit`
    # fixture (`custom_channels`, the same 1023-character key) exists to pin the crate's own
    # permissive behavior on its own terms.
    #
    # It cannot go through `generate_accept_battery`/`generate_reject_battery` like every other
    # candidate above: those helpers only write a candidate when conda's own verdict matches
    # (accepted, for `generate_accept_battery`), and conda genuinely rejects this one (that's the
    # entire point) -- so folding it into `SHARED_ACCEPT` would make the battery itself skip
    # writing it, silently reproducing the exact bug this unconditional write exists to fix
    # (`clear_stale` deletes the previously-written file, nothing recreates it). Named in
    # `tests/condarc_conformance.rs`'s `RUST_ONLY_FIXTURES` (the `Conda`/`OpenApi` checkers are
    # skipped entirely for it, before running anything -- there is no real conda verdict to
    # self-verify against here). Its `expected/*.json` sibling is likewise hand-authored, by
    # `generate_zzz_condarc_expected_fixtures.py`'s `CRATE_ONLY_YAML_KEY_LENGTH_FIXTURES`, not
    # generated from a live conda read.
    crate_only_name = "dict_of_strings_values_accept_key_exceeds_yaml_simple_key_length_limit.json"
    crate_only_doc = {"custom_channels": {"k" * 1023: "val"}}
    (VALID_DIR / crate_only_name).write_text(json.dumps(crate_only_doc, indent=2) + "\n")
    print(f"WROTE   {crate_only_name}  (unconditional -- crate-only fixture, no conda self-check)\n")
    total_written += 1

    print(
        f"=== total: {total_written} fixture(s) written, "
        f"{total_unexpected} skipped ==="
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
