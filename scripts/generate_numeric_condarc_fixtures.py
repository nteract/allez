#!/usr/bin/env python3
"""Generate the exhaustive numeric (`int`/`float`) `.condarc` key fixture
batteries: `numeric_values_accept_*` / `numeric_values_reject_*` (valid
for every plain numeric key) plus two narrower batteries that isolate
the one real behavioral difference between the `int`-typed and
`float`-typed keys: `numeric_values_accept_float_only_*` and
`numeric_values_reject_int_only_*`.

## Scope: which keys this covers

Per `docs/condarc_research.md` §4, exactly 13 `Context` parameters are
declared with a bare, non-tuple, non-enum numeric `element_type` (no
custom `validation=` callable, no nullability, no boolish tuple shape):

  - **`INT_KEYS`** (11, `element_type=int`): `repodata_threads`,
    `fetch_threads`, `default_threads`, `remote_max_retries`,
    `remote_backoff_factor`, `verify_threads`, `execute_threads`,
    `auto_stack`, `verbosity`, `unsatisfiable_hints_check_depth`,
    `number_channel_notices`.
  - **`FLOAT_KEYS`** (2, `element_type=float`): `remote_connect_timeout_secs`,
    `remote_read_timeout_secs`.

Deliberately **excluded**, all for reasons already recorded elsewhere:

  - `local_repodata_ttl` -- declared `(bool, int)`, a *tuple*, so it
    doesn't go through a plain `int(value)`/`float(value)` constructor
    call at all; it routes through `typify_str_no_hint`'s narrower regex
    table instead (`docs/condarc_research.md` §2.1, §8 items 6/11) and
    already has its own dedicated fixture batteries
    (`generate_local_repodata_ttl_fixtures.py` /
    `..._reject_fixtures.py`). Explicitly out of scope per this task.
  - conda-build variables (`bld_path`, `croot`, `anaconda_upload`,
    `conda_build`) and CLI-only variables -- out of scope for
    `conformance/condarc/**` entirely (`docs/condarc_research.md` §8
    item 5); §4's catalog (the source of `INT_KEYS`/`FLOAT_KEYS` above)
    already excludes both categories, so nothing further to filter here.

## Why `int` and `float` need separate treatment at all

Per `docs/condarc_research.md` §2.1's dispatch table, a bare non-tuple,
non-bool `type_hint` goes through `LoadedParameter.typify()` ->
`typify()`'s `elif type_hint is not None` branch: `type_hint(value)`,
i.e. a **plain Python constructor call** (`int(value)` or
`float(value)`), with any raised `ValueError` caught and re-raised as a
clean `TypeCoercionError` -> `CustomValidationError`. `int()` and
`float()` accept an overlapping but *not identical* string/number
vocabulary:

  - Every value `int()` accepts, `float()` also accepts (a JSON/YAML
    integer, an integer-shaped string, `+`/`-` signs, `_`-digit-group
    separators (PEP 515), leading zeros in a string, a bignum of
    arbitrary size, a bare `bool`, or a JSON/YAML float -- `int(3.7) ==
    3`, truncating **toward zero**, not flooring or rounding: `int(-3.7)
    == -3`, *not* `-4`. This truncation-not-flooring direction is a
    common cross-language divergence point and is exercised explicitly
    below.).
  - `float()` additionally accepts genuinely fractional/exponential
    string forms int() rejects outright: decimal points (`"3.5"`,
    `".5"`, `"5."`), scientific notation (`"1e3"`, `"1E3"`, `"1e-3"`),
    and the case-insensitive special tokens `"nan"`/`"inf"`/`"infinity"`
    (optionally signed: `"-inf"`, `"+inf"`) -- **all silently accepted,
    with no post-hoc range/finiteness validation anywhere in `Context`**
    (no numeric key here has a custom `validation=` callable per §3).
    `int()` raises `ValueError` for every one of these (`"invalid
    literal for int() with base 10: ..."`), *except* that a bare
    (unquoted-YAML / already-a-Python-float) `nan`/`inf` value reaching
    `int()` as a real `float` object (not a string) raises a
    *different*, **uncaught** `OverflowError`/keeps `ValueError` --
    itself not reachable from a JSON fixture (JSON has no unquoted
    `Infinity`/`NaN` number literal; that path is YAML-only and out of
    scope for these JSON-only fixtures -- see `check_conda` in
    `tests/condarc_conformance.rs`, which always serializes fixtures via
    `serde_json::to_string` first).
  - Neither `int()` nor `float()` accepts non-base-10 numeric-base
    *strings* (`"0x1A"`, `"0o17"`, `"0b101"` -- despite each looking like
    a valid literal in some language's syntax, base-10-only `int()`/
    `float()` reject all three identically), nor a `complex()`-shaped
    string (`"1+2j"`), nor a doubled/leading/trailing `_` digit
    separator (`"1__000"`, `"_1000"`, `"1000_"` -- PEP 515 requires
    single underscores strictly *between* digits), nor the empty/
    whitespace-only string.

Empirically confirmed (this script's own self-verification, and see
`docs/condarc_research.md` §8 items 7/11's identical finding for the
boolish keys): a raw JSON `null`, or a non-empty JSON array/object,
crashes real conda with an **unhandled** `TypeError`/`AttributeError`
rather than a clean validation error -- for *every* numeric key, `int`-
or `float`-typed alike, since the crash happens in
`LoadedParameter._typify_data_structure`/`.typify()`, upstream of the
`int(value)`/`float(value)` call itself. An *empty* array/object does
**not** crash (same "emptiness, not shape" nuance as the boolish keys)
-- it fails cleanly with `InvalidTypeError` instead. Both outcomes are
still "invalid" for this suite's purposes (exit code is non-zero either
way), so both shapes are included in the reject battery.

## Cross-implementation numeric-storage risk (why several candidates
## deliberately exceed 64-bit range)

Real conda (backed by arbitrary-precision Python `int`/`float`) accepts
integer strings far outside any fixed-width integer type's range --
`i64`/`u64`/`i32`, etc. A conformance suite that only tries "reasonable"
numbers would never catch a hypothetical Rust/other-language
implementation (or a JSON Schema validator with a narrower `integer`
representation) silently truncating, wrapping, erroring, or losing
precision on a value real conda accepts outright. The `numeric_string_
bignum_*` accept candidates below deliberately straddle `i64::MAX`,
`u64::MAX`, and go three orders of magnitude past `f64`'s ~1.8e308 max
finite value (where a real Python `float()` call silently *overflows to
infinity* with **no error raised at all** -- yet another concrete
divergence point worth its own dedicated candidate).

Every candidate (both accept and reject, all four batteries) is verified
empirically against a real `conda` installation before a fixture is
written, for the same self-correcting reason as every other generator
in this directory -- an accept candidate that's unexpectedly *rejected*,
or a reject candidate that's unexpectedly *accepted*, is reported as
SKIPPED rather than silently written to the wrong directory.

Usage:
    python3 scripts/generate_numeric_condarc_fixtures.py

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
INT_KEYS: list[str] = [
    "repodata_threads",
    "fetch_threads",
    "default_threads",
    "remote_max_retries",
    "remote_backoff_factor",
    "verify_threads",
    "execute_threads",
    "auto_stack",
    "verbosity",
    "unsatisfiable_hints_check_depth",
    "number_channel_notices",
]
FLOAT_KEYS: list[str] = [
    "remote_connect_timeout_secs",
    "remote_read_timeout_secs",
]
ALL_NUMERIC_KEYS: list[str] = INT_KEYS + FLOAT_KEYS

# ---------------------------------------------------------------------
# Candidate batteries -- (slug, value) pairs. See module docstring for
# the reasoning behind each group.
# ---------------------------------------------------------------------

# Valid for every key in ALL_NUMERIC_KEYS (both int() and float() accept
# these, though not always to the same effective value -- see the
# truncation-vs-exact-value note on the float candidates below).
SHARED_ACCEPT: list[tuple[str, object]] = [
    ("int_zero", 0),
    ("int_positive", 7),
    ("int_negative", -7),
    ("int_boundary_i32_max", 2147483647),
    ("int_boundary_i32_min", -2147483648),
    ("bool_true", True),
    ("bool_false", False),
    # int(3.7) == 3, int(-3.7) == -3 -- truncates *toward zero*, not
    # floor/round. A real cross-language divergence point: languages
    # whose numeric-coercion floors instead of truncating would produce
    # -4 for the negative case, disagreeing with real conda.
    ("float_with_fraction_truncates_toward_zero_positive", 3.7),
    ("float_with_fraction_truncates_toward_zero_negative", -3.7),
    ("float_zero", 0.0),
    ("numeric_string_plain", "42"),
    ("numeric_string_plus_sign", "+42"),
    ("numeric_string_negative", "-42"),
    # int()/float() both tolerate leading zeros in a *string* (unlike a
    # bare, unquoted JSON/YAML number literal, which JSON's own grammar
    # forbids from having leading zeros in the first place).
    ("numeric_string_leading_zeros", "007"),
    # PEP 515 digit-group separator -- single underscore *between*
    # digits only (see the malformed variants in SHARED_REJECT).
    ("numeric_string_underscored", "1_000"),
    ("numeric_string_whitespace_padded", " 42 "),
    ("numeric_string_whitespace_padded_tabs_and_newlines", "\t\n42\n\t"),
    # Bignum strings straddling fixed-width integer boundaries -- see
    # the module docstring's "cross-implementation numeric-storage risk"
    # section. All of these are ordinary, exactly-representable Python
    # ints, so int()-typed keys keep the exact value; float()-typed
    # keys convert losslessly-enough (well within f64 range) too.
    ("numeric_string_bignum_exceeds_i64_max", "9223372036854775808"),  # i64::MAX + 1
    ("numeric_string_bignum_exceeds_u64_max", "18446744073709551616"),  # u64::MAX + 1
    ("numeric_string_bignum_negative", "-" + "9" * 50),
    # Three orders of magnitude past f64's ~1.8e308 max finite value.
    # int() keeps this as an exact (very large) integer; float() -- per
    # the module docstring -- silently overflows to +inf with *no error
    # raised at all*. Both outcomes are "valid" for real conda; an
    # implementation with a narrower numeric type might reject, wrap, or
    # error where conda does neither.
    ("numeric_string_bignum_exceeds_f64_max_finite", "1" + "0" * 320),
]

# Valid for FLOAT_KEYS only -- float() accepts these; int() raises
# ValueError for every one (see numeric_int_only reject battery below,
# which exercises the int-side rejection of the same strings).
FLOAT_ONLY_ACCEPT: list[tuple[str, object]] = [
    ("string_decimal", "3.5"),
    ("string_decimal_leading_dot", ".5"),
    ("string_decimal_trailing_dot", "5."),
    ("string_negative_decimal", "-3.5"),
    ("string_plus_decimal", "+3.5"),
    ("string_scientific_lower", "1e3"),
    ("string_scientific_upper", "1E3"),
    ("string_scientific_negative_exponent", "1e-3"),
    ("string_scientific_signed", "+1.5e+10"),
    ("string_underscored_decimal", "1_000.5"),
    # nan/inf tokens -- case-insensitive, optionally signed; no
    # finiteness/NaN-rejection validation exists anywhere for these
    # keys (§3 lists no custom validation= callable for any numeric
    # key), so all of these round-trip successfully with no error.
    ("string_nan_lower", "nan"),
    ("string_nan_upper", "NAN"),
    ("string_nan_titlecase", "NaN"),
    ("string_inf_lower", "inf"),
    ("string_inf_upper", "INF"),
    ("string_infinity_word", "Infinity"),
    ("string_negative_infinity", "-inf"),
    ("string_positive_infinity_explicit_sign", "+inf"),
]

# Invalid for every key in ALL_NUMERIC_KEYS -- neither int() nor
# float() accepts any of these (or, for the collection/null shapes,
# the failure happens upstream of either constructor call entirely).
SHARED_REJECT: list[tuple[str, object]] = [
    # Non-base-10 numeric-base literal *strings* -- int()/float() are
    # base-10-only; despite each superficially resembling a valid
    # numeric-base literal in other languages/contexts (and despite
    # unquoted `0x1A` being a legal YAML *int* literal that conda's own
    # YAML loader would resolve natively -- see module docstring, that
    # path requires a bare/unquoted YAML scalar, unreachable from these
    # JSON-serialized fixtures), a quoted JSON *string* is rejected
    # identically for all three bases.
    ("string_hex_literal", "0x1A"),
    ("string_octal_literal", "0o17"),
    ("string_binary_literal", "0b101"),
    ("string_complex_number", "1+2j"),
    ("string_arbitrary_word", "banana"),
    ("string_empty", ""),
    ("string_whitespace_only", "   "),
    # PEP 515 digit-group separator, malformed: doubled, leading, or
    # trailing underscore all raise ValueError for both int() and
    # float() (a single underscore strictly *between* digits, exercised
    # in SHARED_ACCEPT/FLOAT_ONLY_ACCEPT, is the only accepted form).
    ("string_double_underscore", "1__000"),
    ("string_leading_underscore", "_1000"),
    ("string_trailing_underscore", "1000_"),
    # Collection-shape battery (docs/condarc_research.md §8 item 7,
    # re-confirmed here for numeric keys specifically): an *empty*
    # list/dict fails cleanly (InvalidTypeError); a *non-empty* one
    # crashes with an unhandled AttributeError. Both are still
    # "invalid" (non-zero exit) either way.
    ("array_empty", []),
    ("array_nonempty", [1, 2, 3]),
    ("object_empty", {}),
    ("object_nonempty", {"a": 1}),
    # A bare JSON null crashes real conda with an unhandled TypeError
    # (`int()`/`float()` argument must be ... not 'NoneType') rather
    # than a clean validation error -- same crash *shape* as the
    # collection-nonempty case above, different underlying cause.
    ("null_literal", None),
]

# Invalid for INT_KEYS specifically (float()-only-valid strings fed to
# an int()-typed key) -- applied to INT_KEYS only, deliberately
# excluding FLOAT_KEYS so each exploded single-key case is genuinely
# invalid (mirrors FLOAT_ONLY_ACCEPT's candidate set, minus the
# nan/inf/scientific variants already implied by the plain forms below
# being representative).
INT_ONLY_REJECT: list[tuple[str, object]] = [
    ("string_decimal", "3.5"),
    ("string_decimal_leading_dot", ".5"),
    ("string_scientific", "1e3"),
    ("string_underscored_decimal", "1_000.5"),
    ("string_nan", "nan"),
    ("string_inf", "inf"),
    ("string_negative_inf", "-inf"),
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
    keys: list[str],
    candidates: list[tuple[str, object]],
    expect_valid: bool,
) -> tuple[list[str], list[str]]:
    """Applies each (slug, value) candidate to every key in `keys` at
    once (one shared multi-key document per candidate), checks it
    against real conda, and writes a fixture only when the outcome
    matches `expect_valid`. Returns (written, unexpected)."""
    clear_stale(directory, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in candidates:
        doc = {key: value for key in keys}
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
        ("--- numeric_values_accept_* (all 13 numeric keys) ---",
         VALID_DIR, "numeric_values_accept_", ALL_NUMERIC_KEYS, SHARED_ACCEPT, True),
        ("--- numeric_values_accept_float_only_* (2 float keys) ---",
         VALID_DIR, "numeric_values_accept_float_only_", FLOAT_KEYS, FLOAT_ONLY_ACCEPT, True),
        ("--- numeric_values_reject_* (all 13 numeric keys) ---",
         INVALID_DIR, "numeric_values_reject_", ALL_NUMERIC_KEYS, SHARED_REJECT, False),
        ("--- numeric_values_reject_int_only_* (11 int keys) ---",
         INVALID_DIR, "numeric_values_reject_int_only_", INT_KEYS, INT_ONLY_REJECT, False),
    ]

    for label, directory, prefix, keys, candidates, expect_valid in batteries:
        print(f"{label}\n")
        written, unexpected = generate_battery(
            python, directory, prefix, keys, candidates, expect_valid
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
