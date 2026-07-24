.PHONY: help test conformance conformance-conda conformance-crate conformance-openapi regenerate-condarc-fixtures

CONFORMANCE_TEST := cargo test --test condarc_conformance --features conformance-tests
CONFORMANCE_TEST_FILE := tests/condarc_conformance.rs
CONFORMANCE_VALID_DIR := conformance/condarc/valid
CONFORMANCE_INVALID_DIR := conformance/condarc/invalid
CONFORMANCE_EXPECTED_DIR := conformance/condarc/expected

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | sort | awk -F ':.*## ' '{printf "%-24s %s\n", $$1, $$2}'

test: ## Run the full cargo test suite
	cargo test --all

# rstest's #[files(...)] attribute globs conformance/condarc/{valid,invalid}
# at compile time (inside the proc-macro expansion), so cargo has no way to
# know a rebuild is needed when fixture *.json files are added/removed/edited
# without any .rs source changing. Touching the test file forces cargo to
# treat it as changed and re-expand the macro against the current fixture
# set before every conformance run, so newly-added fixtures actually get
# exercised instead of silently running against a stale cached test binary.
# See the "Always invoke via `make conformance*`" section of
# tests/condarc_conformance.rs's module doc comment for the full
# rationale (incl. why this is a deliberate fix, not a build.rs stopgap)
# -- the same touch is duplicated in .github/workflows/ci.yml's
# `conformance` job for the same reason. If you add a new
# `make conformance-*` variant, touch here too.
conformance: ## Run all condarc conformance checks (conda/crate/openapi); each auto-skips if its backend is unavailable
	touch $(CONFORMANCE_TEST_FILE)
	$(CONFORMANCE_TEST) -- --nocapture

conformance-conda: ## Run only the conda-oracle condarc conformance checks (check_conda)
	touch $(CONFORMANCE_TEST_FILE)
	ALLEZ_CONFORMANCE_SKIP_CRATE=1 ALLEZ_CONFORMANCE_SKIP_OPENAPI=1 $(CONFORMANCE_TEST) -- --nocapture

conformance-crate: ## Run only the condarc-crate condarc conformance checks (check_crate; GEN-36, currently always skipped)
	touch $(CONFORMANCE_TEST_FILE)
	ALLEZ_CONFORMANCE_SKIP_CONDA=1 ALLEZ_CONFORMANCE_SKIP_OPENAPI=1 $(CONFORMANCE_TEST) -- --nocapture

conformance-openapi: ## Run only the openapi-schema condarc conformance checks (check_openapi)
	touch $(CONFORMANCE_TEST_FILE)
	ALLEZ_CONFORMANCE_SKIP_CONDA=1 ALLEZ_CONFORMANCE_SKIP_CRATE=1 $(CONFORMANCE_TEST) -- --nocapture

regenerate-condarc-fixtures: ## Delete every condarc fixture (and conformance/condarc/expected/*.json) and rerun all scripts/generate_*.py against a real conda oracle (every fixture, including former hand-authored root-shape ones, is now owned by a generator -- see generate_root_shape_condarc_fixtures.py; conformance/condarc/expected/ is regenerated last, by generate_zzz_condarc_expected_fixtures.py -- see that script for why it must sort last in this loop)
	find $(CONFORMANCE_VALID_DIR) $(CONFORMANCE_INVALID_DIR) $(CONFORMANCE_EXPECTED_DIR) -maxdepth 1 -name '*.json' -delete
	@for script in scripts/generate_*.py; do \
		echo "=== $$script ==="; \
		python3 "$$script" || exit 1; \
	done
	touch $(CONFORMANCE_TEST_FILE)
