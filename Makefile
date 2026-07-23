.PHONY: help test conformance conformance-conda conformance-crate conformance-openapi

CONFORMANCE_TEST := cargo test --test condarc_conformance

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | sort | awk -F ':.*## ' '{printf "%-24s %s\n", $$1, $$2}'

test: ## Run the full cargo test suite
	cargo test --all

conformance: ## Run all condarc conformance checks (conda/crate/openapi); each auto-skips if its backend is unavailable
	$(CONFORMANCE_TEST) -- --nocapture

conformance-conda: ## Run only the conda-oracle condarc conformance checks (check_conda)
	ALLEZ_CONFORMANCE_SKIP_CRATE=1 ALLEZ_CONFORMANCE_SKIP_OPENAPI=1 $(CONFORMANCE_TEST) -- --nocapture

conformance-crate: ## Run only the condarc-crate condarc conformance checks (check_crate; GEN-36, currently always skipped)
	ALLEZ_CONFORMANCE_SKIP_CONDA=1 ALLEZ_CONFORMANCE_SKIP_OPENAPI=1 $(CONFORMANCE_TEST) -- --nocapture

conformance-openapi: ## Run only the openapi-schema condarc conformance checks (check_openapi)
	ALLEZ_CONFORMANCE_SKIP_CONDA=1 ALLEZ_CONFORMANCE_SKIP_CRATE=1 $(CONFORMANCE_TEST) -- --nocapture
