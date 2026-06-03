#!/usr/bin/env python3
"""
Crate versioning helper for AI agents (and humans).

Sources the publish set + tiers from `publish_crates.py` and reads the
current dep graph from `cargo metadata`, so nothing here is maintained
by hand. When a crate is added/removed in `publish_crates.py`, this
script automatically picks it up.

Usage:
    ./scripts/crate_versioning.py list
    ./scripts/crate_versioning.py deps <crate>
    ./scripts/crate_versioning.py dependents <crate> [--transitive]
    ./scripts/crate_versioning.py impact <crate> [--breaking]

Tier semantics (mirrors publish_crates.py). Tier 3 is the only workspace-versioned
cohort; every other tier is independently versioned:
  1 = stable/foundational, rarely changes
  2 = template authoring crates
  3 = core, all share workspace.package.version (move together)
  4 = wallet (SDK, clients, storage), decoupled from the core version

SemVer rules for 0.y.z crates:
  patch (0.y.z -> 0.y.(z+1)) — non-breaking. Dependents auto-pick-up via ^0.y.
  minor (0.y.z -> 0.(y+1).0) — breaking. Every direct dependent must update
                                its pin and republish at least a patch.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

# Reuse the publish list as the single source of truth.
sys.path.insert(0, str(Path(__file__).parent))
from publish_crates import CRATES, TIER_LABELS  # type: ignore[import-not-found]

REPO_ROOT = Path(__file__).resolve().parent.parent
PUBLISH_SET = {name for name, _, _ in CRATES}
TIER_OF = {name: tier for name, _, tier in CRATES}

RED = "\033[0;31m"
GREEN = "\033[0;32m"
YELLOW = "\033[1;33m"
CYAN = "\033[0;36m"
BOLD = "\033[1m"
NC = "\033[0m"


def cargo_metadata():
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=REPO_ROOT, capture_output=True, text=True,
    )
    if result.returncode != 0:
        sys.exit(f"cargo metadata failed:\n{result.stderr}")
    return json.loads(result.stdout)


def build_graph():
    """Return (versions, deps, dev_deps) keyed by crate name, restricted to PUBLISH_SET."""
    md = cargo_metadata()
    versions, deps, dev_deps = {}, {}, {}
    for pkg in md["packages"]:
        name = pkg["name"]
        if name not in PUBLISH_SET:
            continue
        versions[name] = pkg["version"]
        normal, dev = set(), set()
        for d in pkg["dependencies"]:
            if d["name"] not in PUBLISH_SET:
                continue
            kind = d.get("kind") or "normal"
            if kind == "dev":
                dev.add(d["name"])
            else:
                normal.add(d["name"])
        # If something appears both normal and dev, treat as normal.
        dev -= normal
        deps[name] = normal
        dev_deps[name] = dev
    missing = PUBLISH_SET - versions.keys()
    if missing:
        sys.exit(f"Publish-set crates missing from cargo metadata: {sorted(missing)}")
    return versions, deps, dev_deps


def reverse(deps):
    rev = {n: set() for n in deps}
    for n, ds in deps.items():
        for d in ds:
            rev.setdefault(d, set()).add(n)
    return rev


def transitive_dependents(seed: str, rev_deps):
    """Forward-closure of dependents (excludes dev edges)."""
    seen, stack = set(), [seed]
    while stack:
        cur = stack.pop()
        for r in rev_deps.get(cur, ()):
            if r not in seen:
                seen.add(r)
                stack.append(r)
    return seen


def cmd_list(_args):
    versions, _, _ = build_graph()
    print(f"{BOLD}Publish order (from publish_crates.py):{NC}")
    for name, crate_dir, tier in CRATES:
        ver = versions[name]
        print(f"  {name:38} {ver:8}  [{TIER_LABELS[tier]:11}]  {crate_dir}")


def cmd_deps(args):
    versions, deps, dev_deps = build_graph()
    if args.crate not in PUBLISH_SET:
        sys.exit(f"{RED}{args.crate} is not in the publish set.{NC}")
    print(f"{BOLD}{args.crate} {versions[args.crate]} [{TIER_LABELS[TIER_OF[args.crate]]}]{NC}")
    print(f"{BOLD}Direct deps in the publish set:{NC}")
    for d in sorted(deps[args.crate]):
        print(f"  -> {d} {versions[d]} [{TIER_LABELS[TIER_OF[d]]}]")
    if dev_deps[args.crate]:
        print(f"{BOLD}Dev-only deps (do not force downstream bumps):{NC}")
        for d in sorted(dev_deps[args.crate]):
            print(f"  -> {d} {versions[d]} [{TIER_LABELS[TIER_OF[d]]}] [dev]")


def cmd_dependents(args):
    versions, deps, dev_deps = build_graph()
    if args.crate not in PUBLISH_SET:
        sys.exit(f"{RED}{args.crate} is not in the publish set.{NC}")
    rev = reverse(deps)
    rev_dev = reverse(dev_deps)
    print(f"{BOLD}{args.crate} {versions[args.crate]} [{TIER_LABELS[TIER_OF[args.crate]]}]{NC}")
    direct = sorted(rev.get(args.crate, ()))
    print(f"{BOLD}Direct dependents (normal):{NC}" + (" (none)" if not direct else ""))
    for d in direct:
        print(f"  <- {d} {versions[d]} [{TIER_LABELS[TIER_OF[d]]}]")
    direct_dev = sorted(rev_dev.get(args.crate, ()))
    if direct_dev:
        print(f"{BOLD}Direct dependents (dev-only — do not need to bump):{NC}")
        for d in direct_dev:
            print(f"  <- {d} {versions[d]} [{TIER_LABELS[TIER_OF[d]]}] [dev]")
    if args.transitive:
        trans = transitive_dependents(args.crate, rev) - set(direct) - {args.crate}
        print(f"{BOLD}Transitive dependents (via normal deps):{NC}" + (" (none)" if not trans else ""))
        for d in sorted(trans):
            print(f"  <~ {d} {versions[d]} [{TIER_LABELS[TIER_OF[d]]}]")


def cmd_impact(args):
    versions, deps, dev_deps = build_graph()
    target = args.crate
    if target not in PUBLISH_SET:
        sys.exit(f"{RED}{target} is not in the publish set.{NC}")
    rev = reverse(deps)
    rev_dev = reverse(dev_deps)
    cur_ver = versions[target]
    target_tier = TIER_OF[target]

    print(f"{BOLD}Impact analysis: {target} {cur_ver} [{TIER_LABELS[target_tier]}]{NC}")
    if not args.breaking:
        print(f"{GREEN}Non-breaking (patch) bump.{NC}")
        print(f"  {target}: {cur_ver} -> {bump(cur_ver, 'patch')}")
        print(f"  Dependents auto-pick-up via ^0.y constraints — no republish needed")
        print(f"  unless a dependent wants to ship the fix.")
        return

    new_ver = bump(cur_ver, "minor")
    print(f"{YELLOW}Breaking (minor) bump: {cur_ver} -> {new_ver}{NC}")
    print()

    tier3_crates = {n for n, t in TIER_OF.items() if t == 3}

    # Fixed-point: what must republish at a new (minor) version, vs what must
    # update pins + republish (patch min, possibly minor).
    #   - target is "minor" (the breaking change).
    #   - Anything in tier 3 that ends up bumping at all forces the entire
    #     tier 3 cohort to bump minor (workspace.package.version moves).
    #   - Anything with a normal dep on a "minor" crate must update its pin
    #     and republish — at minimum patch, "minor?" if it re-exposes types.
    minor_set = {target}
    while True:
        changed = False
        # 1. workspace rollup
        if minor_set & tier3_crates and not tier3_crates.issubset(minor_set):
            minor_set |= tier3_crates
            changed = True
        # 2. propagate pin-updates: any tier-3 crate that depends on a
        #    minor-bumped crate becomes minor too (workspace will move it anyway).
        for c, ds in deps.items():
            if c in minor_set:
                continue
            if ds & minor_set and TIER_OF[c] == 3:
                minor_set.add(c)
                changed = True
        if not changed:
            break

    # Pin-update set: non-tier-3 crates with a normal dep on anything that
    # is being minor-bumped. They must update the pin and republish at least
    # a patch (or minor if their own API re-exposes the changed types).
    # We do NOT cascade further: patch bumps are absorbed by ^0.y pins, so
    # they don't force more downstream changes. If a user picks minor for one
    # of these, they should re-run impact on that crate.
    pin_update_set = {
        c for c, ds in deps.items()
        if c not in minor_set and TIER_OF[c] != 3 and ds & minor_set
    }

    # Render tier-3 cohort.
    if minor_set & tier3_crates:
        print(f"{BOLD}Tier 3 (core) — all republish at the new workspace version:{NC}")
        ws_ver = workspace_version()
        new_ws = bump(ws_ver, "minor")
        print(f"  Set [workspace.package].version = \"{new_ws}\" in root Cargo.toml.")
        print(f"  Update every tier-3 pin in [workspace.dependencies] "
              f"from \"{trim(ws_ver)}\" -> \"{trim(new_ws)}\".")
        for c in sorted(tier3_crates):
            marker = "*" if c == target else " "
            print(f"  {marker} {c} {versions[c]} -> {new_ws}")
        print()

    # Render independent (non-core) minor bumps (target if independent, plus any
    # other independent crate elevated to minor — typically none unless target is).
    non_t3_minor = sorted(c for c in minor_set if TIER_OF[c] != 3)
    if non_t3_minor:
        print(f"{BOLD}Independent (non-core) — minor (breaking) bump required:{NC}")
        for c in non_t3_minor:
            marker = "*" if c == target else " "
            print(f"  {marker} {c} {versions[c]} -> {bump(versions[c], 'minor')}")
        print()

    # Render pin-update set (independent crates that must republish).
    pin_independent = sorted(c for c in pin_update_set if TIER_OF[c] != 3)
    if pin_independent:
        print(f"{BOLD}Independent (non-core) — must update pin(s) and republish "
              f"(patch min, minor if API re-exposes changed types):{NC}")
        for c in pin_independent:
            cur = versions[c]
            # Which deps of c are bumping?
            bumping_deps = sorted(deps[c] & minor_set)
            pin_hint = ", ".join(f"{d}=\"{trim(bump(versions[d], 'minor'))}\""
                                  for d in bumping_deps[:4])
            if len(bumping_deps) > 4:
                pin_hint += f", … (+{len(bumping_deps) - 4} more)"
            print(f"  {c} {cur} [{TIER_LABELS[TIER_OF[c]]}]")
            print(f"    bump:  {cur} -> {bump(cur, 'patch')} (patch) "
                  f"or {bump(cur, 'minor')} (minor) if API re-exposes")
            print(f"    pins:  {pin_hint}")
        print()

    # Dev-only callouts on the target (informational).
    direct_dev_dependents = sorted(rev_dev.get(target, ()))
    if direct_dev_dependents:
        print(f"{CYAN}Dev-only dependents on {target} "
              f"(no version bump required for these):{NC}")
        for c in direct_dev_dependents:
            print(f"  {c} {versions[c]} [{TIER_LABELS[TIER_OF[c]]}] [dev]")
        print()

    # Suggested workflow.
    print(f"{BOLD}Suggested workflow:{NC}")
    step = 1
    if minor_set & tier3_crates:
        print(f"  {step}. Bump workspace.package.version in root Cargo.toml.")
        step += 1
        print(f"  {step}. Update tier-3 pins in [workspace.dependencies].")
        step += 1
    if non_t3_minor:
        print(f"  {step}. Bump these independent crates in their own Cargo.toml:")
        for c in non_t3_minor:
            print(f"       {c} -> {bump(versions[c], 'minor')}")
        step += 1
    if pin_independent:
        print(f"  {step}. For each independent dependent above, "
              f"update pin(s) and choose patch-vs-minor based on API surface.")
        step += 1
    print(f"  {step}. cargo +nightly-2025-06-25 fmt --all, then "
          f"./scripts/publish_crates.py --dry-run, then --execute.")


def workspace_version() -> str:
    root = REPO_ROOT / "Cargo.toml"
    for line in root.read_text().splitlines():
        line = line.strip()
        if line.startswith("version") and "=" in line:
            return line.split("=", 1)[1].strip().strip('"')
    sys.exit("Could not find [workspace.package].version in root Cargo.toml")


def bump(version: str, kind: str) -> str:
    parts = [int(p) for p in version.split(".")]
    while len(parts) < 3:
        parts.append(0)
    major, minor, patch = parts[:3]
    if kind == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if kind == "minor":
        if major == 0:
            # 0.y.z -> 0.(y+1).0 is the breaking bump under Cargo's pre-1.0 rules.
            return f"0.{minor + 1}.0"
        return f"{major}.{minor + 1}.0"
    sys.exit(f"unknown bump kind: {kind}")


def trim(version: str) -> str:
    """0.32.0 -> 0.32 (the form used in [workspace.dependencies] pins)."""
    parts = version.split(".")
    if parts[0] == "0":
        return ".".join(parts[:2])
    return parts[0]


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = p.add_subparsers(dest="cmd", required=True)

    sp.add_parser("list", help="Show the publish set with versions and tiers.").set_defaults(func=cmd_list)

    pd = sp.add_parser("deps", help="What does this crate depend on (in the publish set)?")
    pd.add_argument("crate")
    pd.set_defaults(func=cmd_deps)

    pr = sp.add_parser("dependents", help="What depends on this crate (in the publish set)?")
    pr.add_argument("crate")
    pr.add_argument("--transitive", action="store_true", help="Include indirect dependents.")
    pr.set_defaults(func=cmd_dependents)

    pi = sp.add_parser("impact", help="Who needs to bump if this crate bumps?")
    pi.add_argument("crate")
    pi.add_argument("--breaking", action="store_true",
                    help="Treat the change as a breaking (minor) bump rather than a patch.")
    pi.set_defaults(func=cmd_impact)

    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
