PYTHON ?= python3
CARGO ?= cargo

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.ONESHELL:
.NOTPARALLEL:
.DEFAULT_GOAL := help

# Print one human-readable timing line per command without changing failures.
define TIMED
$(PYTHON) -c 'import subprocess,time,sys; command=sys.argv[1:]; t=time.monotonic(); p=subprocess.run(command); print(f"timing: {command[0]}: {time.monotonic()-t:.2f}s", flush=True); raise SystemExit(p.returncode)'
endef
# Line-coverage floors, one per collected crate. A crate that is absent from
# this list is not measured: coverage is a guardrail over the crates that carry
# production logic, not a central registry every crate must be entered into.
COVERAGE_FLOORS = intention-transport=75 intention-storage=75 intention-engine=75 \
                  intention-tools=60 intention-config=60 intention-daemon=60 intention-providers=60 \
                  intention-tui=20 intention-client=20

# The components and tools each gate runs, so a gate validates exactly the
# pinned set it uses. Every gate also validates the pinned toolchain, and a gate
# with no pins of its own needs nothing beyond it.
LINT_PINS = --component rustfmt --component clippy
TEST_PINS = --tool cargo-nextest
COVERAGE_PINS = --component llvm-tools-preview --tool cargo-nextest --tool cargo-llvm-cov
DEPS_PINS = --tool cargo-deny --tool cargo-machete
NOTICES_PINS = --tool cargo-about
ALL_PINS = $(LINT_PINS) $(COVERAGE_PINS) $(DEPS_PINS) $(NOTICES_PINS)

.PHONY: help bootstrap-tools tools-check fmt fmt-check notices notices-check lint lint-windows test doctest doc coverage coverage-default coverage-artifacts-clean deps e2e-real-api quick check verify ci ci-lint-arch ci-test ci-coverage-default ci-deps

help: ## List supported M0 targets and mutation behavior.
	@printf '%s\n' \
	  'Non-mutating: tools-check fmt-check notices-check lint lint-windows test doctest doc coverage coverage-default deps quick check verify ci ci-lint-arch ci-test ci-coverage-default ci-deps' \
	  'Mutating/networked: bootstrap-tools fmt notices coverage-artifacts-clean e2e-real-api' \
	  '' \
	  'Use make quick for the fast local loop and make verify before acceptance.'

bootstrap-tools: ## MUTATING/NETWORKED: install exact pinned toolchains and quality tools.
	$(PYTHON) quality/bootstrap_tools.py

tools-check: ## Verify the pinned Rust toolchain, its components, and every pinned quality tool.
	$(PYTHON) quality/check_tools.py $(ALL_PINS)

fmt: ## MUTATING: format all Rust code.
	$(CARGO) fmt --all

fmt-check: ## Verify formatting without changing files.
	$(TIMED) $(CARGO) fmt --all --check

notices: ## MUTATING: regenerate committed third-party notices from the locked graph.
	$(PYTHON) quality/check_tools.py $(NOTICES_PINS)
	$(PYTHON) quality/generate_third_party_notices.py

notices-check: ## Verify committed third-party notices match the locked graph.
	$(PYTHON) quality/check_tools.py $(NOTICES_PINS)
	$(PYTHON) quality/generate_third_party_notices.py --check

lint: ## Run strict Rust and Clippy linting.
	$(PYTHON) quality/check_tools.py $(LINT_PINS)
	$(TIMED) $(CARGO) clippy --workspace --all-targets --locked --all-features -- -Dwarnings

lint-windows: ## Cross-lint the Windows target from Linux (needs the pinned cross target and MinGW).
	$(PYTHON) quality/check_tools.py $(LINT_PINS)
	$(TIMED) $(CARGO) clippy --workspace --all-targets --locked --target x86_64-pc-windows-gnu --all-features -- -Dwarnings

test: ## Run nextest and doctests.
	$(PYTHON) quality/check_tools.py $(TEST_PINS)
	$(TIMED) $(CARGO) nextest run --workspace --all-targets --locked --all-features
	$(TIMED) $(CARGO) test --workspace --doc --locked --all-features

doctest: ## Run the workspace documentation tests.
	$(PYTHON) quality/check_tools.py
	$(TIMED) $(CARGO) test --workspace --doc --locked --all-features

doc: ## Verify Rust documentation without warnings.
	$(PYTHON) quality/check_tools.py
	RUSTDOCFLAGS="-D warnings" $(TIMED) $(CARGO) doc --workspace --no-deps --locked --all-features

coverage: ## Collect one instrumented workspace run and enforce the per-crate line floors.
	$(PYTHON) quality/check_tools.py $(COVERAGE_PINS)
	$(TIMED) $(CARGO) llvm-cov --no-report --workspace --all-targets --locked --all-features nextest
	set -euo pipefail; for entry in $(COVERAGE_FLOORS); do \
	  crate="$${entry%%=*}"; floor="$${entry##*=}"; \
	  $(CARGO) llvm-cov report --package "$$crate" --fail-under-lines "$$floor"; \
	done
	mkdir -p quality/reports
	$(CARGO) llvm-cov report --json --summary-only --output-path quality/reports/coverage-workspace.json

coverage-default: coverage ## Alias for the single-pass coverage gate.
	@true

coverage-artifacts-clean: ## MUTATING: remove generated LLVM coverage build artifacts after coverage passes.
	rm -rf target/llvm-cov-target

deps: ## Run the lockfile anchor, the supply-chain policy, the notices check, and manifest hygiene.
	$(PYTHON) quality/check_tools.py $(DEPS_PINS) $(NOTICES_PINS)
	$(TIMED) $(CARGO) metadata --locked --format-version 1 > /dev/null
	$(TIMED) $(CARGO) deny check
	$(TIMED) $(CARGO) machete --with-metadata --skip-target-dir
	$(PYTHON) quality/generate_third_party_notices.py --check

e2e-real-api: tools-check ## MUTATING/NETWORKED: run ignored real-provider API end-to-end tests (network access required; reads .env when present).
	@if [ -f .env ]; then \
	  while IFS='=' read -r name value; do \
	    case "$$name" in INTENTION_*) ;; *) continue ;; esac; \
	    if [ -z "$$(printenv "$$name" 2>/dev/null || true)" ]; then export "$$name=$$value"; fi; \
	  done < .env; \
	fi
	if [ -z "$${INTENTION_REAL_API_KEY:-}" ] || [ -z "$${INTENTION_REAL_API_MODEL:-}" ]; then
	  printf '%s\n' \
	    'usage: INTENTION_REAL_API_KEY=<secret> INTENTION_REAL_API_MODEL=<model-id> make e2e-real-api' \
	    '' \
	    'Values come from the environment or from a local gitignored .env file' \
	    '(explicit environment values take precedence; .env is never committed).' \
	    '' \
	    'Required:' \
	    '  INTENTION_REAL_API_KEY      provider credential; secret material that is never logged or written to reports' \
	    '  INTENTION_REAL_API_MODEL    provider model identifier' \
	    '' \
	    'Optional:' \
	    '  INTENTION_REAL_API_KIND     provider kind (default: generic-chat-completion-api)' \
	    '  INTENTION_REAL_API_ENDPOINT explicit endpoint override for self-hosted providers' \
	    '' \
	    'This target makes real network calls and is never part of quick, check, verify, or the CI gates.' >&2
	  exit 2
	fi
	mkdir -p quality/reports/real-api-e2e
	export INTENTION_REAL_API_E2E=1
	# Forward the optional selectors only when they carry a value, so the tests
	# observe an absent option instead of an empty-string override.
	if [ -n "$${INTENTION_REAL_API_KIND:-}" ]; then export INTENTION_REAL_API_KIND; else unset INTENTION_REAL_API_KIND; fi
	if [ -n "$${INTENTION_REAL_API_ENDPOINT:-}" ]; then export INTENTION_REAL_API_ENDPOINT; else unset INTENTION_REAL_API_ENDPOINT; fi
	$(CARGO) nextest run --locked --package intention-daemon --test real_api_e2e --run-ignored only --no-capture 2>&1 | tee quality/reports/real-api-e2e/last-run.log

quick: fmt-check lint ## Fast local quality loop.
	$(PYTHON) quality/check_tools.py $(TEST_PINS)
	$(TIMED) $(CARGO) nextest run --workspace --all-targets --locked --all-features

check: tools-check fmt-check lint test doc ## Complete non-mutating source-quality gate.
	@true

verify: check coverage deps ## Full reproducible merge and release gate.
	$(MAKE) coverage-artifacts-clean

ci: verify ## CI alias for one local full gate. GitHub Actions invokes the per-job aliases below in parallel matrix jobs.
	@true

ci-lint-arch: fmt-check lint lint-windows doctest doc ## CI lint job: formatting, native and Windows-target lint, doctests, rustdoc.
	@true

ci-test: test ## CI test job: nextest and doctests.
	@true

ci-coverage-default: coverage-default coverage-artifacts-clean ## CI coverage job: coverage, generated-artifact cleanup.
	@true

ci-deps: deps ## CI dependency job.
	@true
