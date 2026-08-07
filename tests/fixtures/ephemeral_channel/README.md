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
explicit, fixture-resolvable package request. They are no longer
`DEFAULT_PACKAGES`'s own value: GEN-25 changed `DEFAULT_PACKAGES` to
`["python"]`, a documented stopgap pending GEN-30's real default/override
mechanism (see `src/ephemeral/defaults.rs`'s own doc comment) — `python`
does not resolve against this fixture channel, so any test exercising the
zero-packages-falls-back-to-`DEFAULT_PACKAGES` code path must assert the
resulting `unresolvable_package` failure rather than a successful install.

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
