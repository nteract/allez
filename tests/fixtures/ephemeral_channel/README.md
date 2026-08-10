# Ephemeral environment channel fixture

This is a checked-in, network-free conda channel for `file://` integration
tests. Regenerate its package archives and `repodata.json` files with:

```console
python3 scripts/generate_ephemeral_channel_fixture.py
```

The generator uses only the Python standard library. It creates deterministic
legacy `.tar.bz2` conda packages with `info/index.json`, `info/files`, and
`info/paths.json`; checksums in each `repodata.json` are computed from the
archive bytes.

## Default-package candidates

`fixture-default-alpha` `1.0.0` and `fixture-default-beta` `1.0.0` are
dependency-free, noarch root-channel packages meant for tests that need an
explicit, fixture-resolvable package request — including GEN-30's tests of a
`.condarc` `create_default_packages` setting, which name them via a test
`.condarc` rather than any compiled-in list.

No built-in, `allez`-authored default package list exists at all (GEN-30
FR-002 removed `DEFAULT_PACKAGES` outright). An ephemeral environment's
default packages come exclusively from the invoking user's own
`create_default_packages` setting, so the zero-packages case succeeds with an
empty installed set rather than falling back to anything this fixture channel
would have to satisfy.

`fixture-probe` is intentionally not a default-package candidate.

## Priority conflict

Two separate, complete mini-channels expose `fixture-priority`: `priority-a/noarch`
publishes version `1.0.0`, while `priority-b/noarch` publishes version `2.0.0`.

List the `file://` URL for `priority-a` first to assert strict channel
priority selects version `1.0.0`.

## Deliberately corrupt checksum

The root `noarch` channel contains `fixture-corrupt-checksum` `1.0.0`. Its
archive is structurally valid, but both checksum fields in its repodata record
are deliberately all-zero values rather than hashes of its archive. It is the
only intentionally invalid checksum record in this fixture.

## Executable probe

`fixture-probe` `1.0.0` has no dependencies and is published as a real,
non-noarch artifact for every supported target:

| Subdir | Archive payload |
| --- | --- |
| `linux-64` | `bin/fixture-probe`, executable `#!/bin/sh` script |
| `linux-aarch64` | `bin/fixture-probe`, executable `#!/bin/sh` script |
| `osx-arm64` | `bin/fixture-probe`, executable `#!/bin/sh` script |
| `win-64` | `Scripts/fixture-probe.cmd`, `@exit /b 0` batch file |
| `win-arm64` | `Scripts/fixture-probe.cmd`, `@exit /b 0` batch file |

The Unix scripts have archive mode `0755`. This package exists only for the
T017 activation/PATH usability assertion.
