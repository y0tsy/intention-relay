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
.PHONY: help bootstrap-tools tools-check fmt fmt-check notices notices-check lint test docs-check architecture coverage coverage-default coverage-artifacts-clean deps e2e-real-api quick check verify ci ci-lint-arch ci-test ci-coverage-default ci-deps

help: ## List supported M0 targets and mutation behavior.
	@printf '%s\n' \
	  'Non-mutating: tools-check fmt-check notices-check lint test docs-check architecture coverage coverage-default deps quick check verify ci ci-lint-arch ci-test ci-coverage-default ci-deps' \
	  'Mutating/networked: bootstrap-tools fmt notices coverage-artifacts-clean e2e-real-api' \
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

lint: tools-check ## Run strict Rust and Clippy linting.
	$(PYTHON) quality/run_profiles.py lint

test: tools-check ## Run nextest and doctests.
	$(PYTHON) quality/run_profiles.py test
	$(PYTHON) quality/run_profiles.py doctest

docs-check: tools-check ## Verify Rust docs, Markdown links, Mermaid, and secret patterns.
	$(PYTHON) quality/run_profiles.py doc
	$(PYTHON) quality/check_docs.py

architecture: tools-check ## Verify workspace membership and architectural policy.
	$(PYTHON) quality/check_architecture.py

coverage: tools-check ## Collect line coverage and enforce the per-crate tier floors.
	$(PYTHON) quality/run_coverage.py

coverage-default: coverage ## Alias for the single-pass coverage gate.
	@true

coverage-artifacts-clean: ## MUTATING: remove generated LLVM coverage build artifacts after coverage passes.
	rm -rf target/llvm-cov-target

deps: tools-check notices-check ## Run online dependency, license, advisory, notices, and hygiene gates.
	$(PYTHON) quality/run_deps.py

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

quick: tools-check fmt-check lint ## Fast local quality loop.
	$(PYTHON) quality/run_profiles.py test

check: tools-check fmt-check lint test docs-check architecture ## Complete non-mutating source-quality gate.
	@true

verify: check coverage deps ## Full reproducible merge and release gate.
	$(MAKE) coverage-artifacts-clean

ci: verify ## CI alias for one local full gate. GitHub Actions invokes the per-job aliases below in parallel matrix jobs.
	@true

ci-lint-arch: fmt-check lint docs-check architecture ## CI lint/architecture job: formatting, lint, docs, architecture.
	@true

ci-test: test ## CI test job: nextest and doctests.
	@true

ci-coverage-default: coverage-default coverage-artifacts-clean ## CI coverage job: coverage, generated-artifact cleanup.
	@true

ci-deps: deps ## CI dependency job.
	@true
