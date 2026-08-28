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
.PHONY: help bootstrap-tools tools-check fmt fmt-check notices notices-check features isolated-release lint test docs-check architecture coverage coverage-artifacts-clean deps quality-self-test quick check verify ci

help: ## List supported M0 targets and mutation behavior.
	@printf '%s\n' \
	  'Non-mutating: tools-check fmt-check notices-check features isolated-release lint test docs-check architecture coverage deps quality-self-test quick check verify ci' \
	  'Mutating/networked: bootstrap-tools fmt notices coverage-artifacts-clean' \
	  '' \
	  'Use make quick for the fast local loop and make verify before acceptance.'

bootstrap-tools: ## MUTATING/NETWORKED: install exact pinned toolchains and quality tools.
	$(PYTHON) quality/bootstrap_tools.py

tools-check: ## Verify pinned Rust toolchains, components, and quality tools.
	$(PYTHON) quality/check_tools.py

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

isolated-release: tools-check features ## Verify standalone production packages without workspace feature unification.
	$(PYTHON) quality/run_profiles.py isolated-release

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
	$(PYTHON) quality/check_public_api.py

coverage: tools-check ## Collect branch-aware coverage and enforce declared tiers.
	$(PYTHON) quality/run_coverage.py

coverage-artifacts-clean: ## MUTATING: remove generated LLVM coverage build artifacts after coverage passes.
	rm -rf target/llvm-cov-target

deps: tools-check notices-check ## Run online dependency, license, advisory, notices, and hygiene gates.
	$(PYTHON) quality/run_deps.py

quality-self-test: tools-check ## Prove isolated invalid fixtures fail their intended checks.
	$(PYTHON) quality/self_test.py

quick: tools-check fmt-check lint ## Fast default local quality loop.
	$(PYTHON) quality/run_profiles.py test --profile default

check: tools-check fmt-check features isolated-release check-cargo lint test docs-check architecture ## Complete non-mutating source-quality gate.
	@true

verify: check coverage deps ## Full reproducible merge and release gate.
	$(MAKE) coverage-artifacts-clean
	$(MAKE) quality-self-test

ci: metrics-start verify metrics-finish ## CI alias. GitHub Actions must invoke only this target.
	@true

metrics-start: ## Initialize the quality metrics manifest for this run.
	@$(PYTHON) quality/metrics.py start

metrics-finish: ## Finalize the quality metrics manifest preserving the gate result.
	@$(PYTHON) quality/metrics.py finish
