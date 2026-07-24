#!/usr/bin/env python3
"""Generate the exhaustive `boolish_values_accept_*` / `nullable_values_accept_*`
fixture batteries.

Starting point: the "shape" of a `.condarc` that sets every genuinely
*boolish* `Context` parameter to the same value. `conformance/condarc/
valid/boolish_values_accept_bool_true.json` is the seed/reference for
this: every key in `KEYS` below set to JSON `true`.

`KEYS` now spans **three** of the categories from `docs/condarc_research.md`
(the "Full list of bool-like `.condarc` keys" summary that grew out of
this research):

  - **Category A** -- `PLAIN_BOOL_KEYS`: the plain, non-nullable `bool`
    `element_type` parameters (37 of them). These were *not* part of the
    original battery (which only covered the tuple-typed "six boolish
    keys"), but they go through the exact same `boolify()` call as the
    nullable ones below -- just with `nullable=False` -- so the same
    universally-valid tokens apply to them too. Empirically confirmed
    while expanding this script: unlike the tuple-typed keys, an
    unparseable string (e.g. `"banana"`) does *not* trigger the
    `issubclass()` crash bug (§8 item 8) for these -- `issubclass(bool,
    Enum)` is a perfectly legal call, since `bool` (unlike `(bool,
    NoneType)`) is a single class, not a tuple -- so plain-bool keys reject
    cleanly via `CustomValidationError` instead of crashing. This is a
    meaningful, distinct finding worth its own fixtures, not just an
    inference from reading the source.
  - **Category B** -- `NULLABLE_BOOL_KEYS`: the original four `(bool,
    NoneType)` keys (`always_yes`, `report_errors`, `show_channel_urls`,
    `use_only_tar_bz2`).
  - **Category C** -- `ssl_verify`, kept for continuity with the original
    battery (its `(str, bool)` shape and custom filesystem-existence
    validation are unaffected by this expansion).

**`always_softlink` is deliberately excluded from `PLAIN_BOOL_KEYS`'s
membership in the shared "apply one value to every key simultaneously"
battery** (though it's still a real, in-scope Category A key -- see the
full catalog in `docs/condarc_research.md`). Reason: `always_softlink` and
`always_copy` are the subject of the *other* cross-field rule in
`Context.post_build_validation()` (research doc §1.4): if both are
simultaneously truthy, conda raises a `ValidationError` about mutual
exclusivity -- completely unrelated to boolify() coercion. Since every
truthy candidate below (`true`, `"yes"`, `1`, ...) would set *every* key
in `KEYS` to a truthy value simultaneously, including `always_softlink`
alongside `always_copy` would make every truthy-valued "accept" fixture
spuriously fail this unrelated rule. `always_copy` alone already exercises
the identical plain, non-nullable `bool` coercion path, so dropping
`always_softlink` from this shared battery loses no coverage of boolify()
itself; its cross-field behavior is separately covered by
`conformance/condarc/invalid/always_copy_and_softlink.json`.

**`local_repodata_ttl` is deliberately NOT one of `KEYS`** -- it used to
be, but was pulled out (see `docs/condarc_research.md` §8 item 11) because
it doesn't go through `boolify()` at all: its `(bool, int)` element_type
routes through a *different*, narrower regex-based coercion path
(`typify_str_no_hint`) than the rest of `KEYS`. Keeping it in a shared
"apply one value to every boolish key" battery meant several tokens that
are perfectly valid `boolify()` values (`"y"`, `"n"`, `""`, floats,
`"1_000"`, `"1+2j"`, ...) were showing up as *rejected* fixtures purely
because of this one outlier. `local_repodata_ttl` now has its own
dedicated, single-key fixture battery: `scripts/
generate_local_repodata_ttl_fixtures.py` /
`generate_local_repodata_ttl_reject_fixtures.py`.

Why "simultaneously" / why some boolish-looking values are still missing:
`ssl_verify`'s custom `ssl_verify_validation` (a filesystem-existence
check for any string that doesn't itself boolify to a real `bool`)
rejects a few boolify()-adjacent strings (e.g. the string `"null"` -- see
docs/condarc_research.md §8 item 9's addendum). Every candidate below is
verified empirically against a real `conda` installation (the same
oracle `tests/condarc_conformance.rs` uses) before a fixture is written
for it -- so this script is self-correcting if conda's behavior ever
changes, and candidates that fail for any key in `KEYS` are reported as
SKIPPED, not silently included.

**`NULLABLE_ONLY_CANDIDATES`** -- a second, separate battery -- covers the
values that are valid *specifically because* a field is nullable:
`NULL_STRINGS` tokens (`conda/auxlib/type_coercion.py`) that are *not*
also `BOOLISH_FALSE` tokens, namely the string `"null"`, the string `"~"`,
and the string `"\0"` (`"none"` is excluded here since it's already a
`BOOLISH_FALSE` token and therefore already universally valid/covered by
`CANDIDATES` above, regardless of nullability). These three only
successfully coerce (to Python `None`) when `boolify(..., nullable=True)`
is called -- i.e. only for `NULLABLE_BOOL_KEYS` (Category B). Fixtures for
this battery therefore deliberately set **only** the four nullable keys,
not the wider `KEYS` list -- see `generate_boolish_condarc_reject_
fixtures.py` for the mirror-image "these same values, applied only to the
non-nullable keys, are rejected" battery.

Deliberately excluded from `KEYS` (see docs/condarc_research.md §8 item 5):
the "Conda-build Configuration" category (`bld_path`, `croot`,
`anaconda_upload`, `conda_build`) is out of scope for `.condarc`/`conda`
parsing research -- `anaconda_upload` happens to share the exact same
`(bool, NoneType)` shape as the in-scope keys, but is intentionally not
tested here.

Usage:
    python3 scripts/generate_boolish_condarc_fixtures.py

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
FIXTURE_PREFIX = "boolish_values_accept_"
NULLABLE_FIXTURE_PREFIX = "nullable_values_accept_"

# Category A: the plain, non-nullable `bool` element_type `Context`
# parameters -- see docs/condarc_research.md §4 ("Category A"). All 37 of
# these still exist as real, in-scope keys; `always_softlink` is held back
# from `PLAIN_BOOL_KEYS_IN_BATTERY` below (not from the catalog) -- see
# module docstring.
PLAIN_BOOL_KEYS = [
    "override_channels_enabled",
    "add_anaconda_token",
    "allow_non_channel_urls",
    "no_lock",
    "repodata_use_zst",
    "repodata_use_shards",
    "offline",
    "auto_update_conda",
    "force_reinstall",
    "prefix_data_interoperability",
    "allow_softlinks",
    "always_copy",
    "always_softlink",
    "rollback_enabled",
    "extra_safety_checks",
    "shortcuts",
    "non_admin_enabled",
    "separate_format_cache",
    "auto_activate_base",
    "changeps1",
    "json",
    "notify_outdated_conda",
    "quiet",
    "unsatisfiable_hints",
    "envvars_force_uppercase",
    "allow_cycles",
    "allow_conda_downgrades",
    "add_pip_as_python_dependency",
    "debug",
    "trace",
    "dev",
    "enable_private_envs",
    "force_32bit",
    "solver_ignore_timestamps",
    "register_envs",
    "protect_frozen_envs",
    "no_plugins",
]

# See module docstring: excluded from the shared battery only, due to the
# always_copy/always_softlink mutual-exclusivity cross-field rule.
PLAIN_BOOL_KEYS_IN_BATTERY = [k for k in PLAIN_BOOL_KEYS if k != "always_softlink"]

# Category B: the four `(bool, NoneType)` nullable keys.
NULLABLE_BOOL_KEYS = [
    "use_only_tar_bz2",
    "always_yes",
    "report_errors",
    "show_channel_urls",
]

# Category C: kept for continuity with the original battery.
SSL_VERIFY_KEY = "ssl_verify"

# The full shared "apply one value to every key simultaneously" battery:
# Category A (minus always_softlink) + Category B + Category C.
KEYS = PLAIN_BOOL_KEYS_IN_BATTERY + NULLABLE_BOOL_KEYS + [SSL_VERIFY_KEY]

# (slug, value) -- slug must be filesystem-unique even case-folded (macOS'
# default filesystem is case-insensitive), so casing is spelled out in
# words (`_lower`/`_upper`/`_firstupper`) rather than baked into
# case-sensitive-only filename characters.
CANDIDATES: list[tuple[str, object]] = [
    # Exact bool literals.
    ("bool_true", True),
    ("bool_false", False),
    # BOOLISH_TRUE string tokens (conda/auxlib/type_coercion.py) that also
    # survive local_repodata_ttl's narrower regex -- "y" is excluded, see
    # module docstring / research doc §8 item 6.
    ("string_true_lower", "true"),
    ("string_true_upper", "TRUE"),
    ("string_true_firstupper", "True"),
    ("string_yes_lower", "yes"),
    ("string_yes_upper", "YES"),
    ("string_yes_firstupper", "Yes"),
    ("string_on_lower", "on"),
    ("string_on_upper", "ON"),
    ("string_on_firstupper", "On"),
    # BOOLISH_FALSE string tokens.
    ("string_false_lower", "false"),
    ("string_false_upper", "FALSE"),
    ("string_false_firstupper", "False"),
    ("string_no_lower", "no"),
    ("string_no_upper", "NO"),
    ("string_no_firstupper", "No"),
    ("string_off_lower", "off"),
    ("string_off_upper", "OFF"),
    ("string_off_firstupper", "Off"),
    # BOOL_COERCEABLE_TYPES numeric truthiness (any JSON int; bool(int) is
    # plain Python truthiness) -- zero/one/other-truthy/negative patterns.
    ("int_zero", 0),
    ("int_one", 1),
    ("int_two", 2),
    ("int_negative_one", -1),
    # Numeric *strings* -- a distinct code path (str.isnumeric() -> True in
    # boolify()), not merely the same ints stringified.
    ("numeric_string_zero", "0"),
    ("numeric_string_one", "1"),
    ("numeric_string_two", "2"),
    ("numeric_string_negative_one", "-1"),
    # Whitespace-padded boolish tokens. `typify()` (conda/auxlib/
    # type_coercion.py) unconditionally does `value.strip()` on any string
    # value before dispatching to boolify() -- for every one of these five
    # keys' tuple element_types (none of which is the single, exact `str`
    # type, so the whitespace-preserving special case in
    # `_typify_data_structure` never applies) -- so leading/trailing
    # whitespace, including non-space whitespace (tabs/newlines), is
    # silently stripped rather than rejected. Empirically confirmed: see
    # docs/condarc_research.md §8 item 10.
    ("string_whitespace_padded", " true "),
    ("string_whitespace_padded_tabs_and_newlines", "\t\nyes\n\t"),
    # -- Everything below this line used to be excluded (or, before that,
    # rejected) purely because local_repodata_ttl was still in KEYS -- see
    # docs/condarc_research.md §8 item 11. Now that it's split into its
    # own dedicated battery, these are genuinely, universally valid across
    # the five remaining boolify()-based boolish keys. --
    #
    # BOOL_COERCEABLE_TYPES float truthiness (local_repodata_ttl's (bool,
    # int) tuple has no float member and would have rejected this).
    ("float_truthy", 1.5),
    # A bare YAML/JSON null. Not accepted because these keys' tuples
    # include NoneType (only 3 of the 5 do; ssl_verify's is (str, bool)) --
    # accepted because boolify() unconditionally stringifies any non-
    # BOOL_COERCEABLE_TYPES value first, and str(None).lower() == "none",
    # which is itself a BOOLISH_FALSE token. So None round-trips to `False`
    # even for ssl_verify. See docs/condarc_research.md §8 item 11.
    ("null_literal", None),
    # BOOLISH_TRUE/BOOLISH_FALSE's single-letter forms -- boolify() itself
    # has always accepted these; only local_repodata_ttl's narrower regex
    # (which lacks single-letter forms) rejected them.
    ("string_short_true_token", "y"),
    ("string_short_false_token", "n"),
    # The empty string is a BOOLISH_FALSE token in boolify() itself.
    ("string_empty", ""),
    # A complex-number-shaped string: boolify()'s final bool(complex(val))
    # fallback parses it (nonzero -> True) for all five of these keys.
    ("string_complex_number", "1+2j"),
    # PEP 515 underscore-separated numeral: valid complex()/float() syntax,
    # so boolify()'s fallback succeeds cleanly.
    ("string_underscored_int", "1_000"),
    # A decimal-looking string: not a boolify() token, but complex("1.0")
    # succeeds (nonzero -> True) via the same fallback.
    ("string_decimal_string", "1.0"),
]

# NULL_STRINGS (conda/auxlib/type_coercion.py) tokens that are NOT also
# BOOLISH_FALSE tokens -- these only coerce successfully (to Python `None`)
# when boolify() is called with nullable=True, i.e. only for
# `NULLABLE_BOOL_KEYS`. ("none" is deliberately excluded here: it IS a
# BOOLISH_FALSE token too, so it's already universally valid regardless of
# nullability and is covered implicitly by ordinary BOOLISH_FALSE testing,
# not by this nullability-specific battery.) Empirically confirmed while
# building this battery: applying any of these to a plain, non-nullable
# `bool` key (e.g. `debug: "null"`) raises a clean `CustomValidationError`
# ("cannot be boolified") -- not a crash, since `bool` is a single class,
# not a tuple (see module docstring). See `generate_boolish_condarc_reject_
# fixtures.py`'s `NULLABLE_ONLY_REJECT_CANDIDATES` for the mirror-image
# rejection battery applied to the non-nullable keys.
NULLABLE_ONLY_CANDIDATES: list[tuple[str, object]] = [
    ("string_null_token", "null"),
    ("string_tilde_token", "~"),
    ("string_null_byte_token", "\0"),
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


def which(program: str) -> str | None:
    return shutil.which(program)


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

    conda_path = which("conda")
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


def generate_battery(
    python: str,
    keys: list[str],
    candidates: list[tuple[str, object]],
    prefix: str,
    universality_note: str,
) -> tuple[list[str], list[tuple[str, str]]]:
    """Shared driver for a single "apply one candidate value to every key
    in `keys` simultaneously" accept battery. Returns (written, skipped)."""
    stale = sorted(VALID_DIR.glob(f"{prefix}*.json"))
    for path in stale:
        path.unlink()
    if stale:
        print(f"removed {len(stale)} previously-generated {prefix!r} fixture(s)\n")

    written = []
    skipped = []
    for slug, value in candidates:
        doc = {key: value for key in keys}
        is_valid, reason = check_candidate(python, doc)
        filename = f"{prefix}{slug}.json"
        if is_valid:
            out_path = VALID_DIR / filename
            out_path.write_text(json.dumps(doc, indent=2) + "\n")
            written.append(filename)
            print(f"WROTE   {filename}  (value={value!r})")
        else:
            skipped.append((filename, reason))
            print(f"SKIPPED {filename}  (value={value!r}): {reason}")

    print(
        f"\n{len(written)} fixture(s) written, {len(skipped)} candidate(s) "
        f"skipped ({universality_note}).\n"
    )
    return written, skipped


def main() -> int:
    python = find_conda_python()
    print(f"using conda oracle: {python}\n")

    VALID_DIR.mkdir(parents=True, exist_ok=True)

    print(f"--- {FIXTURE_PREFIX}* (Category A + B + C, {len(KEYS)} keys) ---\n")
    generate_battery(
        python,
        KEYS,
        CANDIDATES,
        FIXTURE_PREFIX,
        "not universally valid across all boolish keys",
    )

    print(
        f"--- {NULLABLE_FIXTURE_PREFIX}* (Category B only, "
        f"{len(NULLABLE_BOOL_KEYS)} keys) ---\n"
    )
    generate_battery(
        python,
        NULLABLE_BOOL_KEYS,
        NULLABLE_ONLY_CANDIDATES,
        NULLABLE_FIXTURE_PREFIX,
        "not valid even for the nullable keys",
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
