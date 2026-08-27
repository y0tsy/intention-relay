#!/usr/bin/env python3
"""Validate the phase-aware workspace map and executable crate-boundary policy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_POLICY = ROOT / "quality" / "architecture.toml"
COVERAGE_POLICY = ROOT / "quality" / "coverage.toml"


def fail(message: str) -> None:
    print(f"architecture-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as policy_file:
        return tomllib.load(policy_file)


def cargo_metadata(root: Path) -> dict[str, object]:
    cargo = shutil.which("cargo")
    if cargo is None:
        fail("cargo is unavailable; run make tools-check in the canonical Rust environment")
    completed = subprocess.run(
        [cargo, "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(completed.stdout)
    if not isinstance(metadata, dict):
        fail("cargo metadata must produce an object")
    return metadata


def packages_by_name(metadata: dict[str, object]) -> dict[str, dict[str, object]]:
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        fail("cargo metadata does not contain packages")
    result: dict[str, dict[str, object]] = {}
    for package in packages:
        if not isinstance(package, dict):
            fail("cargo metadata package must be an object")
        name = package.get("name")
        if not isinstance(name, str):
            fail("cargo metadata package has no name")
        result[name] = package
    return result


def source_files(root: Path) -> list[Path]:
    ignored = {"target", ".git", "fixtures", "__pycache__"}
    return [
        path
        for path in root.rglob("*.rs")
        if not any(part in ignored for part in path.relative_to(root).parts)
    ]


def source_texts(root: Path) -> dict[Path, str]:
    return {path: path.read_text(encoding="utf-8") for path in source_files(root)}


def source_files_for_package(package: dict[str, object]) -> list[Path]:
    manifest = package.get("manifest_path")
    if not isinstance(manifest, str):
        fail("cargo metadata package has no manifest path")
    source = Path(manifest).parent / "src"
    return sorted(source.rglob("*.rs")) if source.exists() else []


def string_list(table: dict[str, object], field: str) -> list[str]:
    value = table.get(field)
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        fail(f"policy field {field} must be a string list")
    return value


def check_reasoned_lints(path: Path, text: str) -> list[str]:
    failures: list[str] = []
    for match in re.finditer(r"#!?\[(allow|expect)\((.*?)\)\]", text, flags=re.DOTALL):
        content = match.group(2)
        if "reason" not in content:
            failures.append(f"{path}: {match.group(1)} requires reason = \"...\"")
        if "clippy::all" in content or "clippy::nursery" in content:
            failures.append(f"{path}: broad lint suppression is forbidden")
    return failures


def policy_crates(policy: dict[str, object]) -> dict[str, dict[str, object]]:
    declarations = policy.get("future_crates")
    if not isinstance(declarations, list) or not declarations:
        fail("future crate declarations are required")
    valid_tiers = {"A", "B", "C", "adapter"}
    result: dict[str, dict[str, object]] = {}
    for crate in declarations:
        if not isinstance(crate, dict):
            fail("future crate declaration must be a table")
        name = crate.get("name")
        if not isinstance(name, str) or not name.startswith("intention"):
            fail("future crate names must begin with intention")
        if name in result:
            fail(f"duplicate future crate declaration {name}")
        for field in ("responsibility", "test_target"):
            if not isinstance(crate.get(field), str) or not crate[field]:
                fail(f"future crate {name} requires {field}")
        targets = crate.get("test_targets")
        if not isinstance(targets, list) or not all(isinstance(target, str) and target for target in targets):
            fail(f"future crate {name} requires test_targets as a string list")
        if len(targets) != len(set(targets)):
            fail(f"future crate {name} has duplicate test targets")
        if crate.get("coverage_tier") not in valid_tiers:
            fail(f"future crate {name} has invalid coverage tier")
        result[name] = crate
    return result


def package_dependencies(package: dict[str, object]) -> set[str]:
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        fail(f"package {package.get('name')} has no dependency list")
    result: set[str] = set()
    for dependency in dependencies:
        if not isinstance(dependency, dict):
            fail(f"package {package.get('name')} has an invalid dependency")
        name = dependency.get("name")
        if not isinstance(name, str):
            fail(f"package {package.get('name')} has an unnamed dependency")
        result.add(name)
    return result


def workspace_dependencies(package: dict[str, object], workspace_names: set[str]) -> set[str]:
    return package_dependencies(package) & workspace_names


def dependency_kind(dependency: dict[str, object]) -> str | None:
    kind = dependency.get("kind")
    if kind is None or isinstance(kind, str):
        return kind
    fail(f"package dependency {dependency.get('name')} has an invalid kind")


def production_workspace_dependencies(
    package: dict[str, object], workspace_names: set[str]
) -> set[str]:
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        fail(f"package {package.get('name')} has no dependency list")
    return {
        dependency["name"]
        for dependency in dependencies
        if isinstance(dependency, dict)
        and isinstance(dependency.get("name"), str)
        and dependency_kind(dependency) != "dev"
        and dependency["name"] in workspace_names
    }


def development_workspace_dependencies(
    package: dict[str, object], workspace_names: set[str]
) -> set[str]:
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        fail(f"package {package.get('name')} has no dependency list")
    return {
        dependency["name"]
        for dependency in dependencies
        if isinstance(dependency, dict)
        and isinstance(dependency.get("name"), str)
        and dependency_kind(dependency) == "dev"
        and dependency["name"] in workspace_names
    }


def integration_test_targets(package: dict[str, object]) -> set[str]:
    targets = package.get("targets")
    if not isinstance(targets, list):
        fail(f"package {package.get('name')} has no target list")
    result: set[str] = set()
    for target in targets:
        if not isinstance(target, dict):
            fail(f"package {package.get('name')} has an invalid target")
        name = target.get("name")
        kinds = target.get("kind")
        if not isinstance(name, str) or not isinstance(kinds, list) or not all(isinstance(kind, str) for kind in kinds):
            fail(f"package {package.get('name')} has an invalid target shape")
        if kinds == ["test"]:
            result.add(name)
    return result


def workspace_dependency_graph(packages: dict[str, dict[str, object]]) -> dict[str, set[str]]:
    workspace_names = set(packages)
    return {
        package_name: workspace_dependencies(package, workspace_names)
        for package_name, package in packages.items()
    }


def check_workspace_dependency_cycles(packages: dict[str, dict[str, object]]) -> list[str]:
    graph = workspace_dependency_graph(packages)
    states: dict[str, str] = {name: "unvisited" for name in graph}
    stack: list[str] = []
    failures: list[str] = []

    def visit(package_name: str) -> None:
        states[package_name] = "visiting"
        stack.append(package_name)
        for dependency in sorted(graph[package_name]):
            state = states[dependency]
            if state == "unvisited":
                visit(dependency)
            elif state == "visiting":
                start = stack.index(dependency)
                cycle = stack[start:] + [dependency]
                failures.append(f"workspace dependency cycle: {' -> '.join(cycle)}")
        stack.pop()
        states[package_name] = "visited"

    for package_name in sorted(graph):
        if states[package_name] == "unvisited":
            visit(package_name)
    return failures


def external_dependencies(package: dict[str, object], workspace_names: set[str]) -> set[str]:
    return package_dependencies(package) - workspace_names


def check_dependency_subset(
    package_name: str,
    actual: set[str],
    allowed: set[str],
    boundary: str,
) -> list[str]:
    disallowed = actual - allowed
    if not disallowed:
        return []
    return [
        f"{package_name}: {boundary} dependencies must be a subset of {sorted(allowed)}, "
        f"got disallowed {sorted(disallowed)}"
    ]


def check_source_patterns(
    package_name: str,
    package: dict[str, object],
    texts: dict[Path, str],
    patterns: list[str],
    boundary: str,
) -> list[str]:
    failures: list[str] = []
    for path in source_files_for_package(package):
        text = texts[path]
        for pattern in patterns:
            if package_name == "intention-workspace" and "tests" in path.parts and pattern in {
                "std::env::current_dir",
                "std::env::set_current_dir",
            }:
                continue
            if pattern in text:
                failures.append(f"{path}: {package_name} {boundary} forbids {pattern!r}")
    return failures


def check_phase_policy(
    policy: dict[str, object],
    packages: dict[str, dict[str, object]],
    texts: dict[Path, str],
) -> list[str]:
    state = policy.get("policy")
    if not isinstance(state, dict):
        fail("missing [policy] table")
    phase = state.get("phase")
    if phase not in {"m1", "m2", "m3", "m4", "m5"} or state.get("active_milestone") != phase:
        fail("policy phase and active_milestone must be matching supported milestones")

    declared = policy_crates(policy)
    expected_names = set(declared)
    actual_names = set(packages)
    missing = expected_names - actual_names
    if missing:
        fail(f"{phase.upper()} workspace is missing declared crates: {sorted(missing)}")

    active = string_list(state, "active_production_crates")
    skeletons = string_list(state, "skeleton_crates")
    active_set = set(active)
    skeleton_set = set(skeletons)
    expected_active = {
        "m1": {"intention-types", "intention-domain", "intention-protocol", "intention-config"},
        "m2": {
            "intention-types", "intention-domain", "intention-protocol", "intention-config",
            "intention-transport", "intention-client", "intention", "intention-daemon",
        },
        "m3": {
            "intention-types", "intention-domain", "intention-protocol", "intention-config",
            "intention-transport", "intention-client", "intention", "intention-daemon",
            "intention-application", "intention-runtime", "intention-storage", "intention-storage-sqlite",
        },
        "m4": {
            "intention-types", "intention-domain", "intention-protocol", "intention-config",
            "intention-transport", "intention-client", "intention", "intention-daemon",
            "intention-application", "intention-runtime", "intention-storage", "intention-storage-sqlite",
            "intention-model", "intention-provider-openrouter", "intention-provider-generic-chat",
        },
        "m5": {
            "intention-types", "intention-domain", "intention-protocol", "intention-config",
            "intention-transport", "intention-client", "intention", "intention-daemon",
            "intention-application", "intention-runtime", "intention-storage", "intention-storage-sqlite",
            "intention-model", "intention-provider-openrouter", "intention-provider-generic-chat",
            "intention-tools", "intention-workspace", "intention-hooks",
        },
    }[phase]
    if active_set != expected_active:
        fail(f"{phase.upper()} active production crates must equal the roadmap crate set")
    if len(active) != len(active_set) or len(skeletons) != len(skeleton_set):
        fail("active and skeleton crate lists cannot contain duplicates")
    if active_set & skeleton_set:
        fail(f"an {phase.upper()} crate cannot be both active and a skeleton")

    adapters = policy.get("adapter_boundaries")
    if not isinstance(adapters, dict):
        fail("adapter boundary table is required")
    adapter_set = set(string_list(adapters, "packages"))
    non_production_test = set(string_list(state, "non_production_test_crates"))
    if non_production_test & (active_set | skeleton_set | adapter_set):
        fail("non-production test crates cannot be active, skeleton, or adapters")
    if phase == "m1":
        if active_set | skeleton_set | non_production_test != expected_names:
            fail("M1 active, skeleton, and test-support crate sets must partition the declared workspace")
    else:
        if active_set & adapter_set:
            fail(f"{phase.upper()} adapters cannot be active production crates")
        if active_set | skeleton_set | adapter_set | non_production_test != expected_names:
            fail(f"{phase.upper()} active, skeleton, adapter, and test-support crates must cover the declared workspace")
    if state.get("quality_harness") not in actual_names:
        fail("quality harness must remain a workspace member")

    workspace_policy = policy.get("dependencies")
    external_policy = policy.get("external_dependencies")
    if not isinstance(workspace_policy, dict) or not isinstance(external_policy, dict):
        fail("workspace and external dependency policies are required")
    if set(workspace_policy) != active_set or set(external_policy) != active_set:
        fail("dependency policies must declare exactly the active production crates")

    failures: list[str] = []
    for package_name in active:
        package = packages.get(package_name)
        if package is None:
            fail(f"active crate {package_name} is absent from Cargo metadata")
        if not source_files_for_package(package):
            failures.append(f"{package_name}: active production crate must contain Rust production source")
        declaration = declared.get(package_name)
        if declaration is None:
            failures.append(f"{package_name}: active production crate requires a policy declaration")
            continue
        allowed_workspace = workspace_policy.get(package_name)
        allowed_external = external_policy.get(package_name)
        if not isinstance(allowed_workspace, list) or not all(isinstance(name, str) for name in allowed_workspace):
            fail(f"active crate {package_name} requires a workspace dependency declaration")
        if not isinstance(allowed_external, list) or not all(isinstance(name, str) for name in allowed_external):
            fail(f"active crate {package_name} requires an external dependency declaration")
        actual_workspace = workspace_dependencies(package, actual_names)
        expected_workspace = set(allowed_workspace)
        if actual_workspace != expected_workspace:
            failures.append(
                f"{package_name}: workspace dependencies must equal {sorted(expected_workspace)}, "
                f"got {sorted(actual_workspace)}"
            )
        actual_external = external_dependencies(package, actual_names)
        expected_external = set(allowed_external)
        if actual_external != expected_external:
            failures.append(
                f"{package_name}: external dependencies must equal {sorted(expected_external)}, "
                f"got {sorted(actual_external)}"
            )
        actual_targets = integration_test_targets(package)
        declared_targets = set(declaration["test_targets"])
        if declared_targets != actual_targets:
            failures.append(
                f"{package_name}: declared integration test targets must equal Cargo targets "
                f"{sorted(actual_targets)}, got {sorted(declared_targets)}"
            )
    for package_name in skeleton_set:
        declared_targets = set(declared[package_name]["test_targets"])
        if declared_targets and phase != "m5":
            failures.append(
                f"{package_name}: {phase.upper()} skeleton must not declare integration test targets, "
                f"got {sorted(declared_targets)}"
            )
    non_production_dependencies = policy.get("non_production_test_dependencies")
    if not isinstance(non_production_dependencies, dict):
        fail("non-production test crate dependency policy is required")
    if set(non_production_dependencies) != non_production_test:
        fail("non-production test dependency policy must declare exactly the test-support crates")
    non_production_external_dependencies = policy.get("non_production_test_external_dependencies")
    if not isinstance(non_production_external_dependencies, dict):
        fail("non-production test crate external dependency policy is required")
    if set(non_production_external_dependencies) != non_production_test:
        fail("non-production test external dependency policy must declare exactly the test-support crates")
    for package_name in non_production_test:
        package = packages.get(package_name)
        declaration = declared.get(package_name)
        if package is None or declaration is None:
            fail(f"non-production test crate {package_name} is missing from metadata or policy")
        allowed_dependencies = non_production_dependencies.get(package_name)
        allowed_external_dependencies = non_production_external_dependencies.get(package_name)
        if not isinstance(allowed_dependencies, list) or not all(
            isinstance(name, str) for name in allowed_dependencies
        ):
            fail(f"non-production test crate {package_name} requires workspace dependency policy")
        if not isinstance(allowed_external_dependencies, list) or not all(
            isinstance(name, str) for name in allowed_external_dependencies
        ):
            fail(f"non-production test crate {package_name} requires external dependency policy")
        actual_dependencies = workspace_dependencies(package, actual_names)
        if actual_dependencies != set(allowed_dependencies):
            failures.append(
                f"{package_name}: test-support workspace dependencies must equal "
                f"{sorted(set(allowed_dependencies))}, got {sorted(actual_dependencies)}"
            )
        actual_external_dependencies = external_dependencies(package, actual_names)
        if actual_external_dependencies != set(allowed_external_dependencies):
            failures.append(
                f"{package_name}: test-support external dependencies must equal "
                f"{sorted(set(allowed_external_dependencies))}, got {sorted(actual_external_dependencies)}"
            )
        actual_targets = integration_test_targets(package)
        declared_targets = set(declaration["test_targets"])
        if actual_targets != declared_targets:
            failures.append(
                f"{package_name}: declared integration test targets must equal Cargo targets "
                f"{sorted(actual_targets)}, got {sorted(declared_targets)}"
            )
    return failures

def check_declared_boundaries(
    policy: dict[str, object],
    packages: dict[str, dict[str, object]],
    texts: dict[Path, str],
) -> list[str]:
    workspace_names = set(packages)
    failures: list[str] = []

    adapters = policy.get("adapter_boundaries")
    protocol = policy.get("protocol_boundary")
    composition = policy.get("composition")
    public_contracts = policy.get("public_contracts")
    if not all(isinstance(table, dict) for table in (adapters, protocol, composition, public_contracts)):
        fail("adapter, protocol, composition, and public contract boundary tables are required")
    assert isinstance(adapters, dict)
    assert isinstance(protocol, dict)
    assert isinstance(composition, dict)
    assert isinstance(public_contracts, dict)

    adapter_packages = string_list(adapters, "packages")
    adapter_workspace = set(string_list(adapters, "allowed_workspace_dependencies"))
    adapter_test_workspace = set(string_list(adapters, "allowed_test_workspace_dependencies"))
    adapter_external = set(string_list(adapters, "allowed_external_dependencies"))
    adapter_sources = string_list(adapters, "forbidden_source_patterns")
    for package_name in adapter_packages:
        if package_name not in packages:
            fail(f"adapter boundary references missing package {package_name}")
        package = packages[package_name]
        failures.extend(check_dependency_subset(
            package_name,
            production_workspace_dependencies(package, workspace_names),
            adapter_workspace,
            "adapter production workspace",
        ))
        failures.extend(check_dependency_subset(
            package_name,
            development_workspace_dependencies(package, workspace_names) - adapter_workspace,
            adapter_test_workspace,
            "adapter test workspace",
        ))
        failures.extend(check_dependency_subset(
            package_name,
            external_dependencies(package, workspace_names),
            adapter_external,
            "adapter external",
        ))
        failures.extend(check_source_patterns(package_name, package, texts, adapter_sources, "adapter boundary"))

    protocol_name = protocol.get("package")
    if not isinstance(protocol_name, str) or protocol_name not in packages:
        fail("protocol boundary requires an existing package")
    protocol_package = packages[protocol_name]
    failures.extend(check_dependency_subset(
        protocol_name,
        workspace_dependencies(protocol_package, workspace_names),
        set(string_list(protocol, "allowed_workspace_dependencies")),
        "protocol workspace",
    ))
    failures.extend(check_dependency_subset(
        protocol_name,
        external_dependencies(protocol_package, workspace_names),
        set(string_list(protocol, "allowed_external_dependencies")),
        "protocol external",
    ))
    failures.extend(check_source_patterns(
        protocol_name,
        protocol_package,
        texts,
        string_list(protocol, "forbidden_source_patterns"),
        "protocol boundary",
    ))

    root_package = composition.get("root_package")
    concrete = string_list(composition, "concrete_implementation_crates")
    if not isinstance(root_package, str) or root_package not in packages:
        fail("composition boundary requires an existing root_package")
    concrete_set = set(concrete)
    for package_name, package in packages.items():
        if package_name == root_package:
            continue
        selected = workspace_dependencies(package, workspace_names) & concrete_set
        if selected:
            failures.append(
                f"{package_name}: only {root_package} may select concrete implementations, got {sorted(selected)}"
            )
        for concrete_name in concrete:
            namespace = concrete_name.replace("-", "_") + "::"
            failures.extend(check_source_patterns(
                package_name,
                package,
                texts,
                [namespace],
                f"composition ownership outside {root_package}",
            ))

    sdk_patterns = string_list(public_contracts, "provider_sdk_resource_patterns")
    active = string_list(policy["policy"], "active_production_crates")
    provider_owners = {
        "intention-provider-openrouter": {"openrouter_rs::"},
        "intention-provider-generic-chat": {"async_openai::"},
    }
    for package_name in active:
        allowed_private_sdk = provider_owners.get(package_name, set())
        failures.extend(check_source_patterns(
            package_name,
            packages[package_name],
            texts,
            [pattern for pattern in sdk_patterns if pattern not in allowed_private_sdk],
            "provider SDK/resource boundary",
        ))
    return failures


def check_provider_sdk_ownership(
    policy: dict[str, object],
    packages: dict[str, dict[str, object]],
    texts: dict[Path, str],
) -> list[str]:
    public_contracts = policy.get("public_contracts")
    if not isinstance(public_contracts, dict):
        fail("public contract boundary table is required")
    sdk_patterns = set(string_list(public_contracts, "provider_sdk_resource_patterns"))
    owners = {
        "async_openai::": "intention-provider-generic-chat",
        "openrouter_rs::": "intention-provider-openrouter",
    }
    failures: list[str] = []
    for path, text in texts.items():
        package_name = path.parts[path.parts.index("crates") + 1] if "crates" in path.parts else None
        for pattern in sdk_patterns:
            if pattern not in text:
                continue
            owner = owners.get(pattern)
            if owner is None or package_name != owner:
                failures.append(f"{path}: provider SDK namespace {pattern!r} is allowed only in {owner or 'no crate'} private implementation")
    return failures


def check_coverage_policy(root: Path, architecture: dict[str, object]) -> list[str]:
    coverage = load_toml(root / "quality" / "coverage.toml")
    state = coverage.get("policy")
    tiers = coverage.get("crate_tiers")
    architecture_state = architecture.get("policy")
    if not isinstance(state, dict) or not isinstance(architecture_state, dict):
        return ["coverage and architecture policies require policy tables"]
    phase = architecture_state.get("phase")
    if state.get("phase") != phase:
        return ["coverage policy phase must equal architecture policy phase"]
    if not isinstance(tiers, dict):
        return ["coverage policy requires [crate_tiers]"]
    active = architecture_state["active_production_crates"]
    if state.get("production_crates") != active:
        return ["coverage production crates must equal active production crate list"]
    declared = policy_crates(architecture)
    failures: list[str] = []
    if set(tiers) != set(active):
        failures.append("coverage tiers must declare exactly the active production crates")
    for name in active:
        expected_tier = declared[name]["coverage_tier"]
        if expected_tier not in {"A", "B", "C"}:
            failures.append(f"active crate {name} must have a numeric coverage tier")
        elif tiers.get(name) != expected_tier:
            failures.append(f"coverage tier for {name} must be {expected_tier}")
    return failures

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    arguments = parser.parse_args()

    root = arguments.root.resolve()
    policy = load_toml(arguments.policy)
    metadata = cargo_metadata(root)
    packages = packages_by_name(metadata)
    texts = source_texts(root)
    failures = check_phase_policy(policy, packages, texts)
    failures.extend(check_workspace_dependency_cycles(packages))
    failures.extend(check_declared_boundaries(policy, packages, texts))
    failures.extend(check_provider_sdk_ownership(policy, packages, texts))
    failures.extend(check_coverage_policy(root, policy))

    forbidden = policy.get("forbidden")
    if not isinstance(forbidden, dict):
        fail("missing [forbidden] table")
    patterns = string_list(forbidden, "source_patterns")
    string_list(forbidden, "public_resource_patterns")

    for path, text in texts.items():
        for pattern in patterns:
            if "tests" in path.parts and path.parts[path.parts.index("crates") + 1] == "intention-workspace" and pattern in {
                "std::env::current_dir",
                "std::env::set_current_dir",
            }:
                continue
            if pattern in text:
                if pattern == "std::process::exit" and re.search(
                    r"#!?\[allow\([^]]*clippy::exit[^]]*reason\s*=\s*\"[^\"]+\"[^]]*\)\]",
                    text,
                    re.DOTALL,
                ):
                    continue
                failures.append(f"{path}: forbidden source pattern {pattern!r}")
        failures.extend(check_reasoned_lints(path, text))

    active = policy["policy"]["active_production_crates"]
    for package_name in active:
        for path in source_files_for_package(packages[package_name]):
            text = texts[path]
            if re.search(
                r"pub\s+(?:struct|enum)\s+\w+\s*\{[^}]*\b(?:credential|api_key|token|password)\s*:\s*String",
                text,
                re.DOTALL,
            ):
                failures.append(f"{path}: public DTO cannot contain an unredacted secret field")

    if failures:
        fail("\n".join(failures))
    print("architecture-check: phase-aware workspace, dependency, DTO, adapter, protocol, and composition policies are valid")


if __name__ == "__main__":
    main()
