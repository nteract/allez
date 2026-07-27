#!/usr/bin/env python3
"""Generate the `plain_string_values_accept_*` / `plain_string_values_reject_*`
fixture battery for the 8 `Context` parameters declared with a plain,
single, non-tuple, non-nullable, non-enum `str` `element_type` and **no**
custom `validation=` callable at all (docs/condarc_research.md §4's
catalog): `console`, `default_activation_env`, `env_prompt`,
`error_upload_url`, `root_prefix` (alias: `root_dir`), `solver` (alias:
`experimental_solver`), `subdir`, `target_prefix_override`.

(`channel_alias` is the 9th, and only other, member of this "plain str
element_type" catalog bucket, but it has its own custom
`validation=channel_alias_validation` callable and its own dedicated,
already-complete fixture battery -- see
`scripts/generate_channel_alias_condarc_fixtures.py`'s module docstring.
Not touched by this script.)

## Why these 8 behave identically to each other

None of the 8 declares a custom `validation=` callable (§3), so each
goes through *exactly* the same coercion path as every other bare,
non-tuple, non-bool `element_type` (§2.1's dispatch table, "a single
concrete type T (not a tuple), T otherwise" row): `LoadedParameter.
_typify_data_structure` special-cases `isinstance(value, str) and
issubclass(type_hint, str)` to skip `typify()` (and its whitespace
`.strip()`) entirely for an already-`str` raw value -- so a JSON string
round-trips **byte-for-byte unchanged, including leading/trailing
whitespace** (unlike almost every other string-shaped key in this
codebase, e.g. `default_python`'s `(str, NoneType)` *tuple* element_type,
which does NOT hit this special case). For any *other* JSON scalar type
(`bool`, `int`, `float`, `null`), `typify()` calls the plain `str(value)`
constructor, which never raises for any JSON-representable scalar:
`str(True) == "True"`, `str(False) == "False"`, `str(7) == "7"`,
`str(-7) == "-7"`, `str(3.14) == "3.14"`, `str(None) == "None"`. Since
none of these 8 keys has any further semantic constraint (no regex, no
enum, no filesystem check), **every JSON scalar value is unconditionally
accepted** for all 8 at once.

An empty JSON array/object raw value fails cleanly with `InvalidTypeError`
(`_typify_data_structure` never reaches `str(value)` at all -- the raw
merged value is the wrong outer container shape for a scalar
`PrimitiveParameter`). A *non-empty* array/object instead crashes with an
unhandled `AttributeError: 'YamlRawParameter' object has no attribute
'typify'` -- the same upstream bug already documented in
`docs/condarc_research.md` §8 item 7 for boolish/numeric keys (an
emptiness distinction, not a list-vs-dict one). Both outcomes are still
correctly "invalid" (non-zero exit) for this suite's purposes, so both
shapes are exercised by the reject battery below, mirroring
`channel_alias`'s already-existing `*_reject_array_empty/array_nonempty/
object_empty/object_nonempty.json` fixtures' shape (the exact same
underlying crash/clean-reject boundary, empirically re-confirmed for
these 8 keys specifically -- see module docstring's usage note).

## Aliases: `experimental_solver` / `root_dir`

`solver`'s alias `experimental_solver` and `root_prefix`'s alias
`root_dir` route through the *identical* underlying `ParameterLoader`
coercion path as their canonical name (aliases only affect which raw
key(s) a `ParameterLoader` looks for in a source, not how the matched
value is subsequently coerced) -- so no separate alias-specific
coercion fixtures are generated here; the schema wiring for the alias
*names themselves* is handled directly in `docs/condarc_openapi.json`
(via `x-conda-aliases`), not via fixtures.

Deliberately **not** included in any shared-battery fixture here: a
canonical name and its alias set together in the same document (e.g.
`{"solver": "x", "experimental_solver": "x"}`), since that would trigger
conda's *separate* `MultipleKeysError` alias-collision rule
(docs/condarc_research.md §1.3) -- a cross-cutting concern spanning all
20 aliased-parameter pairs cataloged in
`scripts/generate_alias_multiplekeys_condarc_fixtures.py`, not something
specific to this bucket's coercion behavior.

Every candidate (both batteries) is verified empirically against a real
`conda` installation before a fixture is written -- an accept candidate
that's unexpectedly *rejected*, or a reject candidate that's unexpectedly
*accepted*, is reported as SKIPPED rather than silently written to the
wrong directory, so this script is self-correcting exactly like its
siblings (`generate_channel_alias_condarc_fixtures.py`,
`generate_numeric_condarc_fixtures.py`).

Usage:
    python3 scripts/generate_plain_string_condarc_fixtures.py

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
# Key catalog -- see module docstring's "Why these 8 behave identically"
# section. Canonical names only; aliases (`experimental_solver`,
# `root_dir`) are deliberately excluded from these shared-battery
# fixtures -- see the "Aliases" section above.
# ---------------------------------------------------------------------
KEYS: list[str] = [
    "console",
    "default_activation_env",
    "env_prompt",
    "error_upload_url",
    "root_prefix",
    "solver",
    "subdir",
    "target_prefix_override",
]

# ---------------------------------------------------------------------
# Candidate batteries -- (slug, value) pairs.
# ---------------------------------------------------------------------

# Valid for every key in KEYS -- any JSON scalar coerces via a bare
# str(value) call (or is preserved byte-for-byte if already a str); see
# module docstring.
ACCEPT_CANDIDATES: list[tuple[str, object]] = [
    ("bool_true", True),
    ("bool_false", False),
    ("int_zero", 0),
    ("int_positive", 7),
    ("int_negative", -7),
    ("float_positive", 3.14),
    ("float_negative", -3.14),
    ("null_literal", None),
    ("string_plain", "hello"),
    ("string_empty", ""),
    ("string_whitespace_only", "   "),
    # Whitespace is NOT stripped for these keys (element_type is the
    # single, exact `str` class -- see module docstring) -- this
    # candidate's whole point is to exist as its own fixture so a
    # future implementation that *does* strip whitespace would fail
    # `assert_conda_expected_representation`'s byte-for-byte comparison
    # even though this accept/reject-only schema can't itself detect
    # that (see final report's "what can't be modeled" note).
    ("string_whitespace_padded", "  hello  "),
    ("string_unicode", "héllo wörld 日本語"),
    ("string_long", "x" * 5000),
    # Float-magnitude notation-switching battery (GEN-?? review feedback on
    # `python_repr_float` in crates/condarc/src/coerce/strings.rs): conda's
    # `str(float)` conversion is CPython's `repr(float)`, which switches from
    # fixed-point to `d.ddde+NN`/`d.ddde-NN` scientific notation once the
    # value's decimal-point position falls outside `-4 < decpt <= 16`
    # (CPython's `format_float_short`, mode 'r'). Rust's own `f64::to_string()`
    # never does this -- it always prints full fixed-point digits -- so this
    # existing battery's `float_positive`/`float_negative` pair (3.14/-3.14)
    # accidentally never exercised that divergence: those magnitudes sit well
    # inside the fixed-point range on both sides. Every candidate below is
    # picked to sit *at* or *across* one of the two switch-over boundaries
    # (10**16 on the large side, 10**-4 on the small side), verified against
    # real conda exactly like every other candidate in this file.
    (
        "float_scientific_boundary_1e16",
        1e16,
    ),  # decpt=17 (>16) -- conda: "1e+16"; the reviewer's own example.
    ("float_scientific_1e17", 1e17),  # decpt=18 -- conda: "1e+17".
    ("float_scientific_negative_1e16", -1e16),  # sign must survive the switch: "-1e+16".
    (
        "float_scientific_huge_fractional_mantissa",
        1.5e300,
    ),  # fractional (non-single-digit) mantissa in scientific form: "1.5e+300".
    ("float_scientific_small_1e_minus5", 1e-5),  # decpt=-4 (<=-4) -- conda: "1e-05".
    ("float_scientific_negative_small_1e_minus5", -1e-5),  # conda: "-1e-05".
    (
        "float_boundary_1e15_stays_fixed",
        1e15,
    ),  # decpt=16 (not >16) -- one order of magnitude *inside* the large-side
    # boundary; conda: "1000000000000000.0" (fixed, with the forced ".0").
    (
        "float_boundary_1e_minus4_stays_fixed",
        1e-4,
    ),  # decpt=-3 (not <=-4) -- one order of magnitude *inside* the small-side
    # boundary; conda: "0.0001" (fixed, no forced trailing zero needed).
    (
        "float_boundary_near_1e16_integral_stays_fixed",
        9999999999999998.0,
    ),  # largest-magnitude integral float below 10**16; conda: still fixed
    # ("9999999999999998.0"), unlike 1e16 one candidate above.
    ("float_zero", 0.0),  # conda: "0.0" -- sanity check, not itself divergent.
    ("float_negative_zero", -0.0),  # conda: "-0.0" -- sign must be preserved on zero too.
    # Remaining branch/sub-branch/sign combinations from crates/condarc/src/coerce/strings.rs's
    # `tests` module (`python_repr_float_*`/`shortest_digits_and_exponent_*`) not already
    # covered above -- mirrored here 1:1 so every case that's unit-tested against
    # `python_repr_float` directly is *also* verified end-to-end against a real conda oracle
    # through this battery, not just the crate's own (self-consistent, but conda-independent)
    # reasoning about what `repr(float)` ought to do. `f64::NAN`/`f64::INFINITY`/
    # `f64::NEG_INFINITY` are deliberately excluded: strict JSON has no numeric token for them
    # (Python's `json.dumps` would emit the non-standard `NaN`/`Infinity`/`-Infinity` bare
    # words, which -- unquoted -- are plain YAML *strings*, not floats, defeating the point of
    # a fixture meant to exercise `RawValue::Float`), so this file's JSON-fixture-via-YAML
    # pipeline (`check_candidate`'s `json.dumps(doc)`) cannot represent them at all; the unit
    # tests remain the only coverage for that pair of branches, same as for the two
    # structurally-unreachable `unreachable!()` branches in `shortest_digits_and_exponent`.
    ("float_one", 1.0),  # digits="1", decpt=1==len(1) -- zero-iteration whole-number padding.
    ("float_decimal_point_inside_digits", 3.5),  # decpt=1 < len=2, no padding/forced ".0".
    ("float_decimal_point_inside_digits_negative", -3.5),  # same, negative sign.
    (
        "float_decimal_point_inside_digits_wide",
        123.456,
    ),  # decpt=3 < len=6 -- a wider digit string landing the point mid-string.
    ("float_boundary_1e_minus4_stays_fixed_negative", -1e-4),  # -1e-4 boundary pair, negative
    # sign (positive already covered by `float_boundary_1e_minus4_stays_fixed` above).
    ("float_scientific_huge_fractional_mantissa_negative", -1.5e300),  # 1.5e300's negative-sign
    # pair (+value/+exponent already covered above).
    ("float_scientific_small_magnitude_1e_minus10", 1e-10),  # magnitude==10 exactly -- the
    # exponent-zero-padding "false" branch boundary (1e-5's magnitude=5 hits "true").
    ("float_scientific_multi_digit_negative_exponent", 1.5e-10),  # multi-digit mantissa x
    # negative exponent, the one {mantissa-width}x{exponent-sign} combination the boundary
    # candidates above don't already cover on their own.
    ("float_scientific_multi_digit_negative_exponent_negative", -1.5e-10),  # same, negative sign.
    ("float_fixed_fractional_decpt_zero", 0.5),  # decpt=0 -- zero-iteration leading-zero loop.
    ("float_fixed_fractional_decpt_negative_one", 0.05),  # decpt=-1 -- one leading zero.
    ("float_fixed_fractional_decpt_negative_two", 0.005),  # decpt=-2 -- two leading zeros.
    ("float_fixed_whole_number_zero_padding", 123.0),  # decpt=3==len(3) -- zero-iteration
    # whole-number padding loop, at a small/legible magnitude (see also `float_one` above and
    # `float_boundary_near_1e16_integral_stays_fixed`'s much larger instance of this same edge).
    ("float_fixed_whole_number_zero_padding_negative", -123.0),  # same, negative sign.
    ("float_fixed_whole_number_with_padding", 100.0),  # decpt=3 > len=1 -- a small,
    # legible instance of the whole-number padding loop actually running (2 iterations; see
    # also `float_boundary_1e15_stays_fixed` above for a much larger one, 15 iterations).
    ("float_fixed_whole_number_with_padding_negative", -100.0),  # same, negative sign.
]

# Invalid for every key in KEYS -- collection-shape battery
# (docs/condarc_research.md §8 item 7): an *empty* list/dict fails
# cleanly (InvalidTypeError); a *non-empty* one crashes with an
# unhandled AttributeError. Both are still "invalid" (non-zero exit)
# either way.
REJECT_CANDIDATES: list[tuple[str, object]] = [
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


def generate_battery(
    python: str,
    directory: Path,
    prefix: str,
    candidates: list[tuple[str, object]],
    expect_valid: bool,
) -> tuple[list[str], list[str]]:
    """Applies each (slug, value) candidate to every key in KEYS at once
    (one shared multi-key document per candidate), checks it against
    real conda, and writes a fixture only when the outcome matches
    `expect_valid`. Returns (written, unexpected)."""
    clear_stale(directory, prefix)

    written: list[str] = []
    unexpected: list[str] = []
    for slug, value in candidates:
        doc = {key: value for key in KEYS}
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
                f"by conda ({reason[:150]})"
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
            "--- plain_string_values_accept_* (8 keys) ---",
            VALID_DIR,
            "plain_string_values_accept_",
            ACCEPT_CANDIDATES,
            True,
        ),
        (
            "--- plain_string_values_reject_* (8 keys) ---",
            INVALID_DIR,
            "plain_string_values_reject_",
            REJECT_CANDIDATES,
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

    print(f"=== total: {total_written} fixture(s) written, {total_unexpected} skipped ===")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
