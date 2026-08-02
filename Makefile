PYTHON ?= python3
CARGO ?= cargo

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.ONESHELL:
.NOTPARALLEL:
.DEFAULT_GOAL := help

.PHONY: help bootstrap-tools tools-check fmt fmt-check notices notices-check features lint test docs-check architecture coverage deps quality-self-test quick check verify ci

help: ## List supported M0 targets and mutation behavior.
	@printf '%s\n' \
	  'Non-mutating: tools-check fmt-check notices-check features lint test docs-check architecture coverage deps quality-self-test quick check verify ci' \
	  'Mutating/networked: bootstrap-tools fmt notices' \
	  '' \
	  'Use make quick for the fast local loop and make verify before acceptance.'

bootstrap-tools: ## MUTATING/NETWORKED: install exact pinned toolchains and quality tools.
	$(PYTHON) quality/bootstrap_tools.py

tools-check: ## Verify pinned Rust toolchains, components, and quality tools.
	$(PYTHON) quality/check_tools.py

fmt: ## MUTATING: format all Rust code.
	$(CARGO) fmt --all

fmt-check: ## Verify formatting without changing files.
	$(CARGO) fmt --all --check

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
	$(PYTHON) quality/run_profiles.py check

docs-check: tools-check ## Verify Rust docs, Markdown links, Mermaid, and secret patterns.
	$(PYTHON) quality/run_profiles.py doc
	$(PYTHON) quality/check_docs.py

architecture: tools-check ## Verify workspace membership and architectural policy.
	$(PYTHON) quality/check_architecture.py
	$(PYTHON) quality/check_public_api.py

coverage: tools-check ## Collect branch-aware coverage and enforce declared tiers.
	$(PYTHON) quality/run_coverage.py

deps: tools-check notices-check ## Run online dependency, license, advisory, notices, and hygiene gates.
	$(CARGO) metadata --locked --format-version 1 > /dev/null
	$(PYTHON) quality/check_deny_policy.py
	$(CARGO) deny check
	$(CARGO) audit
	$(PYTHON) quality/run_profiles.py udeps
	$(CARGO) machete --with-metadata --skip-target-dir
	$(CARGO) outdated --workspace --root-deps-only --exit-code 1

quality-self-test: tools-check ## Prove isolated invalid fixtures fail their intended checks.
	$(PYTHON) quality/self_test.py

quick: tools-check fmt-check lint ## Fast default local quality loop.
	$(CARGO) nextest run --workspace --all-targets --locked

check: tools-check fmt-check features check-cargo lint test docs-check architecture ## Complete non-mutating source-quality gate.
	@true

verify: check coverage deps quality-self-test ## Full reproducible merge and release gate.
	@true

ci: verify ## CI alias. GitHub Actions must invoke only this target.
	@true
