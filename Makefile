PYTHON ?= python3
CARGO ?= cargo

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.ONESHELL:
.NOTPARALLEL:
.DEFAULT_GOAL := help

# Keep phase timing in one human-readable manifest without changing failures.
define TIMED
$(PYTHON) -c 'import subprocess,time,sys; command=sys.argv[1:]; t=time.monotonic(); p=subprocess.run(command); print(f"timing: {command[0]}: {time.monotonic()-t:.2f}s", flush=True); raise SystemExit(p.returncode)'
endef
.PHONY: help bootstrap-tools tools-check fmt fmt-check notices notices-check features lint test docs-check architecture coverage coverage-default coverage-no-default coverage-all coverage-artifacts-clean deps quality-self-test quality-self-test-in-place quick check verify ci ci-source ci-lint-arch ci-test ci-coverage ci-coverage-default ci-coverage-no-default ci-coverage-all ci-selftest ci-deps metrics-start metrics-finish metrics-start-source metrics-start-lint-arch metrics-start-test metrics-start-coverage metrics-start-coverage-default metrics-start-coverage-no-default metrics-start-coverage-all metrics-start-deps metrics-start-selftest metrics-finish-source metrics-finish-lint-arch metrics-finish-test metrics-finish-coverage metrics-finish-coverage-default metrics-finish-coverage-no-default metrics-finish-coverage-all metrics-finish-deps metrics-finish-selftest

help: ## List supported M0 targets and mutation behavior.
	@printf '%s\n' \
	  'Non-mutating: tools-check fmt-check notices-check features lint test docs-check architecture coverage coverage-default coverage-no-default coverage-all deps quality-self-test quality-self-test-in-place quick check verify ci ci-source ci-lint-arch ci-test ci-coverage ci-coverage-default ci-coverage-no-default ci-coverage-all ci-selftest ci-deps' \
	  'Mutating/networked: bootstrap-tools fmt notices coverage-artifacts-clean' \
	  '' \
	  'Use make quick for the fast local loop and make verify before acceptance.'

bootstrap-tools: ## MUTATING/NETWORKED: install exact pinned toolchains and quality tools.
	$(PYTHON) quality/bootstrap_tools.py

tools-check: ## Verify pinned Rust toolchains, components, and quality tools (CI scopes via CI_TOOLS_SCOPE).
	$(PYTHON) quality/check_tools.py --scope $${CI_TOOLS_SCOPE:-all}

fmt: ## MUTATING: format all Rust code.
	$(CARGO) fmt --all

fmt-check: ## Verify formatting without changing files.
	$(TIMED) $(CARGO) fmt --all --check

notices: tools-check ## MUTATING: regenerate committed third-party notices from the locked graph.
	$(PYTHON) quality/generate_third_party_notices.py

notices-check: tools-check ## Verify committed third-party notices match the locked graph.
	$(PYTHON) quality/generate_third_party_notices.py --check

features: ## Verify machine-readable required feature profiles.
	$(PYTHON) quality/check_features.py --print

lint: tools-check ## Run strict Rust and Clippy linting for all feature profiles.
	$(PYTHON) quality/run_profiles.py lint

test: tools-check ## Run nextest and doctests for all feature profiles.
	$(PYTHON) quality/run_profiles.py test
	$(PYTHON) quality/run_profiles.py doctest

check-cargo: tools-check ## Run Cargo check for all feature profiles.
	$(TIMED) $(PYTHON) quality/run_profiles.py check

docs-check: tools-check ## Verify Rust docs, Markdown links, Mermaid, and secret patterns.
	$(PYTHON) quality/run_profiles.py doc
	$(PYTHON) quality/check_docs.py

architecture: tools-check ## Verify workspace membership and architectural policy.
	$(PYTHON) quality/check_architecture.py

coverage: tools-check ## Collect branch-aware coverage and enforce declared tiers.
	$(PYTHON) quality/run_coverage.py

coverage-default: tools-check ## Collect branch-aware coverage for the default profile.
	$(PYTHON) quality/run_coverage.py --profile default

coverage-no-default: tools-check ## Collect branch-aware coverage for the no-default profile.
	$(PYTHON) quality/run_coverage.py --profile no_default

coverage-all: tools-check ## Collect branch-aware coverage for the all-features profile.
	$(PYTHON) quality/run_coverage.py --profile all

coverage-artifacts-clean: ## MUTATING: remove generated LLVM coverage build artifacts after coverage passes.
	rm -rf target/llvm-cov-target

deps: tools-check notices-check ## Run online dependency, license, advisory, notices, and hygiene gates.
	$(PYTHON) quality/run_deps.py

quality-self-test: tools-check ## Prove isolated invalid fixtures fail their intended checks.
	$(PYTHON) quality/self_test.py

quality-self-test-in-place: tools-check ## CI in-place fixture check with git-restore scoping (reuses warm Cargo artifacts).
	$(PYTHON) quality/self_test.py --in-place

quick: tools-check fmt-check lint ## Fast default local quality loop.
	$(PYTHON) quality/run_profiles.py test --profile default

check: tools-check fmt-check features check-cargo lint test docs-check architecture ## Complete non-mutating source-quality gate.
	@true

verify: check coverage deps ## Full reproducible merge and release gate.
	$(MAKE) coverage-artifacts-clean
	$(MAKE) quality-self-test

ci: metrics-start verify metrics-finish ## CI alias for one local full gate. GitHub Actions invokes the per-job aliases below in parallel matrix jobs.
	@true

ci-source: metrics-start-source check metrics-finish-source ## CI source-quality job (local convenience): metrics, check, metrics.
	@true

ci-lint-arch: metrics-start-lint-arch fmt-check features lint docs-check architecture metrics-finish-lint-arch ## CI lint/architecture job: metrics, formatting, features, lint, docs, architecture, metrics.
	@true

ci-test: metrics-start-test check-cargo test metrics-finish-test ## CI test job: metrics, checks, nextest and doctests, metrics.
	@true

ci-coverage: metrics-start-coverage coverage coverage-artifacts-clean metrics-finish-coverage ## CI coverage job (all profiles, local convenience): metrics, coverage, generated-artifact cleanup, metrics.
	@true

ci-coverage-default: metrics-start-coverage-default coverage-default coverage-artifacts-clean metrics-finish-coverage-default ## CI coverage job for the default profile.
	@true

ci-coverage-no-default: metrics-start-coverage-no-default coverage-no-default coverage-artifacts-clean metrics-finish-coverage-no-default ## CI coverage job for the no-default profile.
	@true

ci-coverage-all: metrics-start-coverage-all coverage-all coverage-artifacts-clean metrics-finish-coverage-all ## CI coverage job for the all-features profile.
	@true

ci-selftest: metrics-start-selftest quality-self-test-in-place metrics-finish-selftest ## CI self-test job: metrics, in-place fixture check, metrics.
	@true

ci-deps: metrics-start-deps deps metrics-finish-deps ## CI dependency job: metrics, deps, metrics.
	@true

metrics-start: ## Initialize the quality metrics manifest for this run.
	@$(PYTHON) quality/metrics.py start

metrics-finish: ## Finalize the quality metrics manifest preserving the gate result.
	@$(PYTHON) quality/metrics.py finish

metrics-start-source metrics-start-lint-arch metrics-start-test metrics-start-coverage metrics-start-coverage-default metrics-start-coverage-no-default metrics-start-coverage-all metrics-start-deps metrics-start-selftest:
	@$(PYTHON) quality/metrics.py start --job $(patsubst metrics-start-%,%,$@)

metrics-finish-source metrics-finish-lint-arch metrics-finish-test metrics-finish-coverage metrics-finish-coverage-default metrics-finish-coverage-no-default metrics-finish-coverage-all metrics-finish-deps metrics-finish-selftest:
	@$(PYTHON) quality/metrics.py finish --job $(patsubst metrics-finish-%,%,$@)
