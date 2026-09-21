#!/usr/bin/env python3
"""
Pre-tag release gate for the Ootle workspace.

Answers one question: *is this tree safe to tag?* Everything a tag sets off is
irreversible — crates.io and npm publishes are immutable, a docker tag and a
release are public the moment they exist — so every check that can run before
the tag runs here.

    ./scripts/release_check.py                 # check HEAD against the last v* tag
    ./scripts/release_check.py --since v0.40.0 # against a specific baseline
    ./scripts/release_check.py --offline       # skip registry lookups
    ./scripts/release_check.py --package       # also `cargo package` what it can

Checks, in order:

  1. **Tree** — clean, on a release branch, in sync with the remote.
  2. **Version** — `[workspace.package].version` is not already tagged here or on
     the remote.
  3. **Changelog** — the version has an entry.
  4. **Crate bumps** — every publish-set crate whose files changed since the
     baseline must carry a version crates.io does not already hold. A changed
     crate on a spent version ships its change under a number that announces
     nothing, and `publish_crates.py` silently skips it.
  5. **Publish preflight** — the publish order is topological and every
     publish-set pin resolves against crates.io ∪ what this release publishes.
     This is the check `cargo publish --dry-run` cannot do: a dry run resolves
     only against the registry, so it fails on any dep whose new version this
     release has not published yet. That failure is expected and says nothing
     about whether the real publish will work; this section does.
  6. **npm bumps** — same rule as crates, for the packages the tag publishes.
  7. **Protocol version** — a touched activation schedule needs its epochs set
     deliberately, not inherited from the last release.

Exit status is 1 if any check FAILs. Warnings never fail the run; they are the
things only a human can settle.
"""

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from crate_versioning import (  # type: ignore[import-not-found]
    BOLD,
    CYAN,
    GREEN,
    NC,
    RED,
    YELLOW,
    breaks_pin,
    cargo_metadata,
    fetch_releases,
    vkey,
)
from publish_crates import CRATES, check_order  # type: ignore[import-not-found]

REPO_ROOT = Path(__file__).resolve().parent.parent

# The npm packages a `v*` tag publishes (npm_publish.yml, npm_publish_ootle_wasm.yml).
# ootle-wasm is absent: its version is its crate version, so the crate checks cover it.
# (package directory, registry name)
NPM_PACKAGES = [
    ("bindings", "@tari-project/ootle-ts-bindings"),
    ("clients/javascript/wallet_daemon_client", "@tari-project/wallet_jrpc_client"),
    ("clients/javascript/indexer_client", "@tari-project/indexer-client"),
]

# Branches a release may be cut from. Kept as a set so the eventual collapse of the
# main/development split is a one-line edit.
RELEASE_BRANCHES = {"main"}

# A change here moves a network's protocol activation schedule, which is an epoch
# decision per network — never a value carried over from the previous release.
ACTIVATION_SCHEDULE = "crates/engine_types/src/protocol_version.rs"


class Report:
    """Collects section results so the summary can be printed once, at the end."""

    def __init__(self):
        self.failures = []
        self.warnings = []

    def section(self, title: str):
        print(f"\n{BOLD}{title}{NC}")

    def ok(self, msg: str):
        print(f"  {GREEN}✓{NC} {msg}")

    def warn(self, msg: str):
        print(f"  {YELLOW}!{NC} {msg}")
        self.warnings.append(msg)

    def fail(self, msg: str):
        print(f"  {RED}✗{NC} {msg}")
        self.failures.append(msg)

    def note(self, msg: str):
        print(f"    {msg}")


def git(*args, check=True) -> str:
    result = subprocess.run(
        ["git", *args], cwd=REPO_ROOT, capture_output=True, text=True,
    )
    if check and result.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed:\n{result.stderr}")
    return result.stdout.strip()


def last_release_tag() -> str:
    """The highest v* tag by version, or '' when there is none.

    Highest, not nearest: releases are cut on the release branch, so the previous
    release is often not an ancestor of a development HEAD and `git describe`
    would reach past it to an older tag — widening the diff and flagging crates
    the last release already carried.
    """
    tags = git("tag", "--list", "v[0-9]*", "--sort=-v:refname").splitlines()
    # Release tags only: a pre-release ("v0.50.0-pre.0") sorts above the release
    # line it anticipates and would make every crate look changed since it.
    releases = [t for t in tags if t[1:].replace(".", "").isdigit()]
    return releases[0] if releases else ""


def changed_paths(base: str) -> list:
    """Every path that differs between `base` and HEAD."""
    return git("diff", "--name-only", f"{base}..HEAD").splitlines()


def touched(paths: list, prefix: str) -> bool:
    return any(p == prefix or p.startswith(prefix.rstrip("/") + "/") for p in paths)


def workspace_version() -> str:
    for line in (REPO_ROOT / "Cargo.toml").read_text().splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    sys.exit("could not read [workspace.package] version from Cargo.toml")


def npm_versions(name: str):
    """Every published version of an npm package, or None when the lookup failed.

    Asks for the abbreviated packument — the full one carries every version's
    complete manifest, which for these packages is megabytes of noise.
    """
    url = f"https://registry.npmjs.org/{urllib.request.quote(name, safe='@')}"
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.npm.install-v1+json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return list(json.load(resp).get("versions", {}))
    except urllib.error.HTTPError as e:
        return [] if e.code == 404 else None
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
        return None


def local_versions() -> dict:
    """{crate: version} for the publish set, from cargo metadata."""
    names = {name for name, _, _ in CRATES}
    return {
        pkg["name"]: pkg["version"]
        for pkg in cargo_metadata()["packages"]
        if pkg["name"] in names
    }


def publish_set_pins() -> dict:
    """{crate: {dep: req}} over publish-set edges, dev-deps excluded.

    Dev-deps are excluded because they are not part of what a consumer resolves:
    a dev-only edge in the publish set is declared path-only precisely so it does
    not reach the registry (see publish_crates.check_order).
    """
    names = {name for name, _, _ in CRATES}
    pins = {}
    for pkg in cargo_metadata()["packages"]:
        if pkg["name"] not in names:
            continue
        pins[pkg["name"]] = {
            dep["name"]: dep["req"]
            for dep in pkg["dependencies"]
            if dep["name"] in names and (dep.get("kind") or "normal") != "dev"
        }
    return pins


def satisfies(req: str, version: str) -> bool:
    """Does `version` satisfy a caret/bare requirement `req`?

    Only the pin shapes this workspace uses are handled — "0.41", "^0.41",
    "0.41.2" — which are all caret semantics. Anything else answers True rather
    than inventing a failure the gate cannot justify.
    """
    req = req.strip()
    if req.startswith("^"):
        req = req[1:]
    if not req or not req[0].isdigit():
        return True
    want, have = vkey(req), vkey(version)
    if vkey(version) < vkey(req):
        return False
    # Caret on 0.y.z is compatible only within the same 0.y.
    return not breaks_pin(version, req) if want[0] == 0 else want[0] == have[0]


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------

def check_tree(rep: Report, args):
    rep.section("Tree")
    branch = git("rev-parse", "--abbrev-ref", "HEAD")
    if branch in RELEASE_BRANCHES:
        rep.ok(f"on {branch}")
    else:
        rep.warn(f"on {branch}, not a release branch ({', '.join(sorted(RELEASE_BRANCHES))})")

    if git("status", "--porcelain"):
        rep.fail("working tree is dirty — a tag must name a committed state")
    else:
        rep.ok("working tree is clean")

    if args.offline:
        return
    subprocess.run(["git", "fetch", "--quiet", "--tags", "origin"], cwd=REPO_ROOT, check=False)
    upstream = git("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}", check=False)
    if not upstream:
        rep.warn("no upstream branch — cannot confirm HEAD is pushed")
        return
    ahead_behind = git("rev-list", "--left-right", "--count", f"{upstream}...HEAD", check=False)
    behind, ahead = (ahead_behind.split() + ["0", "0"])[:2]
    if ahead != "0":
        rep.fail(f"HEAD is {ahead} commit(s) ahead of {upstream} — push before tagging")
    elif behind != "0":
        rep.fail(f"HEAD is {behind} commit(s) behind {upstream} — the tag would miss them")
    else:
        rep.ok(f"in sync with {upstream}")


def check_version(rep: Report, args, version: str) -> None:
    rep.section(f"Version — v{version}")
    tag = f"v{version}"
    if git("tag", "-l", tag):
        rep.fail(f"{tag} already exists locally — bump [workspace.package].version or delete the tag")
    else:
        rep.ok(f"{tag} is free locally")

    if args.offline:
        return
    remote = git("ls-remote", "--tags", "origin", tag, check=False)
    if remote:
        rep.fail(f"{tag} already exists on origin — that release is cut; this needs a new version")
    else:
        rep.ok(f"{tag} is free on origin")


def check_changelog(rep: Report, version: str) -> None:
    rep.section("Changelog")
    path = REPO_ROOT / "CHANGELOG.md"
    if not path.exists():
        rep.fail("CHANGELOG.md is missing")
        return
    if f"## [{version}]" in path.read_text():
        rep.ok(f"CHANGELOG.md has an entry for {version}")
    else:
        rep.fail(f"CHANGELOG.md has no `## [{version}]` entry — write it before tagging")


def check_crate_bumps(rep: Report, args, base: str, releases: dict) -> set:
    """Every changed publish-set crate must sit on a version crates.io lacks.

    Returns the set of crates this release will publish (changed or otherwise
    unreleased), which the publish preflight resolves pins against.
    """
    rep.section(f"Crate versions (changed since {base})")
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", base, "HEAD"], cwd=REPO_ROOT, capture_output=True,
    )
    if ancestor.returncode != 0:
        rep.warn(f"{base} is not an ancestor of HEAD — it carries commits this tag would not")
        rep.note("Merge the release branch back first, or pass --since <the right baseline>.")
    paths = changed_paths(base)
    pending, stale = set(), []
    for name, directory, _ in CRATES:
        rel = releases[name]
        if not rel.taken:
            pending.add(name)
        if not touched(paths, directory):
            continue
        if not rel.known:
            rep.warn(f"{name} changed; crates.io lookup failed — verify by hand")
            continue
        if rel.taken:
            stale.append((name, rel.local))
        # A changed crate on an unreleased version is already covered: the pending
        # release ships the change under a number nobody can yet be pinned to.

    for name, local in stale:
        rep.fail(f"{name} changed since {base} but {local} is already on crates.io — needs a bump")
    if stale:
        rep.note(f"Run: ./scripts/crate_versioning.py impact {stale[0][0]} [--breaking]")
    if not stale:
        rep.ok("every changed crate carries an unreleased version")
    if pending:
        rep.ok(f"{len(pending)} crate(s) will publish: {', '.join(sorted(pending))}")
    else:
        rep.warn("no crate has an unreleased version — this tag publishes nothing to crates.io")
    return pending


def check_publish_preflight(rep: Report, args, releases: dict, pending: set) -> None:
    rep.section("Publish preflight")
    violations = check_order()
    if violations:
        for dependent, dependency in violations:
            rep.fail(f"publish order: {dependent} is published before its dependency {dependency}")
        rep.note("Fix the order of CRATES in publish_crates.py.")
    else:
        rep.ok("publish order is topological")

    # A pin only has to resolve against what will exist *after* this release: the
    # registry's current contents plus everything this tag publishes. cargo's own
    # --dry-run cannot see the second half, which is why it fails here routinely.
    unresolved = []
    for crate, pins in publish_set_pins().items():
        for dep, req in pins.items():
            rel = releases[dep]
            candidates = [v for v, yanked in (rel.published or []) if not yanked]
            if dep in pending:
                candidates.append(rel.local)
            if not rel.known:
                continue
            if not any(satisfies(req, v) for v in candidates):
                unresolved.append((crate, dep, req, rel.local))
    for crate, dep, req, local in unresolved:
        rep.fail(f"{crate} pins {dep} = \"{req}\", which nothing satisfies (tree has {local})")
    if not unresolved:
        rep.ok("every publish-set pin resolves against crates.io + this release")

    if args.package:
        packageable = [
            name for name, _, _ in CRATES
            if name in pending and not (publish_set_pins()[name].keys() & pending)
        ]
        if not packageable:
            rep.warn("no crate can be `cargo package`d yet — every pending crate depends on another")
            rep.note("Expected: they package once their deps are on crates.io, mid-publish.")
        for name in packageable:
            result = subprocess.run(
                ["cargo", "package", "-p", name, "--no-verify", "--allow-dirty", "--quiet"],
                cwd=REPO_ROOT, capture_output=True, text=True,
            )
            if result.returncode == 0:
                rep.ok(f"cargo package {name}")
            else:
                rep.fail(f"cargo package {name} failed:\n{result.stderr.strip()[:400]}")


def check_npm_bumps(rep: Report, args, base: str) -> None:
    rep.section(f"npm packages (changed since {base})")
    paths = changed_paths(base)
    interesting = [(d, n) for d, n in NPM_PACKAGES if touched(paths, d)]
    if not interesting:
        rep.ok("no published npm package changed")
        return

    local = {}
    for directory, name in interesting:
        manifest = json.loads((REPO_ROOT / directory / "package.json").read_text())
        local[name] = manifest["version"]

    published = {}
    if not args.offline:
        with ThreadPoolExecutor(max_workers=len(interesting)) as pool:
            published = dict(zip(
                [n for _, n in interesting],
                pool.map(npm_versions, [n for _, n in interesting]),
            ))

    for directory, name in interesting:
        version = local[name]
        live = published.get(name)
        if args.offline or live is None:
            rep.warn(f"{name} ({directory}) changed at {version} — registry not checked")
        elif version in live:
            rep.fail(f"{name} changed but {version} is already on npm — bump {directory}/package.json")
        else:
            rep.ok(f"{name} {version} is unpublished and will publish on the tag")

    # The bindings are the source the downstream SDK is generated against, so a
    # change there is a change every ootle.ts consumer eventually sees.
    if any(d == "bindings" for d, _ in interesting):
        rep.warn("bindings changed — after the release, bump the ootle.ts catalog pin (see checklists/release.md)")


def check_protocol_version(rep: Report, base: str) -> None:
    rep.section("Protocol version")
    if ACTIVATION_SCHEDULE in changed_paths(base):
        rep.warn(f"{ACTIVATION_SCHEDULE} changed — confirm each network's activation epoch is set for THIS release")
        rep.note("An epoch carried over from the previous release activates in the past or never.")
    else:
        rep.ok("activation schedule unchanged")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--since", metavar="REF", help="Baseline to diff against (default: the last v* tag).")
    p.add_argument("--offline", action="store_true", help="Skip crates.io/npm/remote lookups.")
    p.add_argument("--package", action="store_true", help="Also `cargo package` the crates that can be.")
    args = p.parse_args()

    base = args.since or last_release_tag()
    if not base:
        sys.exit("no v* tag found — pass --since <ref> to set a baseline")

    version = workspace_version()
    print(f"{BOLD}Release check{NC} — {CYAN}v{version}{NC} against {CYAN}{base}{NC}")

    rep = Report()
    releases = fetch_releases(local_versions(), offline=args.offline)

    check_tree(rep, args)
    check_version(rep, args, version)
    check_changelog(rep, version)
    pending = check_crate_bumps(rep, args, base, releases)
    check_publish_preflight(rep, args, releases, pending)
    check_npm_bumps(rep, args, base)
    check_protocol_version(rep, base)

    print()
    if rep.failures:
        print(f"{RED}{BOLD}NOT READY TO TAG{NC} — {len(rep.failures)} blocker(s), {len(rep.warnings)} warning(s)")
        return 1
    print(f"{GREEN}{BOLD}READY TO TAG{NC} — 0 blockers, {len(rep.warnings)} warning(s)")
    print(f"  git tag -a v{version} -m 'v{version} — <headline>' && git push origin v{version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
