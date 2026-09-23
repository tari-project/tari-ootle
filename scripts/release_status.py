#!/usr/bin/env python3
"""
Post-tag release status for the Ootle workspace.

A `v*` tag fans out into six independent pipelines (binaries, FFI libs, docker,
npm, docs, crates.io) and none of them tells you about the others. This gathers
all of it into one screen, so the decision the release owner actually makes —
*is the draft complete enough to publish?* — is one command instead of six tabs.

    ./scripts/release_status.py                 # the highest release tag
    ./scripts/release_status.py v0.41.0
    ./scripts/release_status.py --watch         # re-render until nothing is pending

Publishing the draft is the point of no return for the docs: docs-deploy.yml
fires on `release: published` and the developer docs render the wallet downloads
of the newest *non-draft* release. A release published with a platform's binaries
missing is a docs page offering nothing for that platform — v0.41.0 shipped that
way for Windows, and a best-effort build leg that fails does not fail the tag run.

Requires: gh (authenticated), git.
"""

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import base64
from crate_versioning import BOLD, CYAN, GREEN, NC, RED, YELLOW  # type: ignore[import-not-found]
from publish_crates import CRATES, published_versions  # type: ignore[import-not-found]
from release_check import NPM_PACKAGES, local_versions, npm_versions  # type: ignore[import-not-found]

REPO_ROOT = Path(__file__).resolve().parent.parent
GH_REPO = "tari-project/tari-ootle"

# Platforms ffi_libs.yml builds. Every one is required: ootle-go vendors a lib per
# platform and cannot ship a gap.
FFI_PLATFORMS = [
    "macos-arm64", "macos-x86_64", "linux-x86_64", "linux-arm64",
    "linux-x86_64-musl", "windows-x64",
]

OK, MISSING, OPTIONAL = "ok", "missing", "optional"


def sh(*args, check=True):
    result = subprocess.run(args, cwd=REPO_ROOT, capture_output=True, text=True)
    if check and result.returncode != 0:
        sys.exit(f"{' '.join(args)} failed:\n{result.stderr}")
    return result.stdout.strip()


def gh_json(*args):
    """Run a gh command that prints JSON; None when gh fails (not authed, no release)."""
    result = subprocess.run(["gh", *args], cwd=REPO_ROOT, capture_output=True, text=True)
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return None


def highest_release_tag() -> str:
    tags = sh("git", "tag", "--list", "v[0-9]*", "--sort=-v:refname").splitlines()
    releases = [t for t in tags if t[1:].replace(".", "").isdigit()]
    if not releases:
        sys.exit("no release tag found — pass one explicitly")
    return releases[0]


def mark(state: str) -> str:
    return {OK: f"{GREEN}✓{NC}", MISSING: f"{RED}✗{NC}", OPTIONAL: f"{YELLOW}!{NC}"}[state]


def binary_platforms() -> list:
    """[(platform, required)] from the binary build matrix.

    `best_effort` legs are allowed to fail without failing the tag run, so their
    absence is a warning; everything else is a blocker for publishing the draft.
    """
    matrix = json.loads((REPO_ROOT / ".github/workflows/build_binaries.json").read_text())
    return [(e["name"], not e.get("best_effort", False)) for e in matrix]


def report_release(tag: str, sha: str) -> int:
    print(f"\n{BOLD}GitHub release{NC}")
    release = gh_json(
        "release", "view", tag, "--repo", GH_REPO,
        "--json", "isDraft,publishedAt,assets,tagName",
    )
    if release is None:
        print(f"  {RED}✗{NC} no release for {tag} yet (the tag run creates the draft)")
        return 1

    state = "draft" if release["isDraft"] else "PUBLISHED"
    colour = YELLOW if release["isDraft"] else GREEN
    print(f"  {colour}{state}{NC}  {len(release['assets'])} asset(s)")

    names = [a["name"] for a in release["assets"]]
    blockers = 0

    # An asset whose embedded short sha is not the tag's belongs to another build of
    # the same version — a re-cut release line. Consumers that select assets by name
    # (ootle-go's vendor_release.sh) have to disambiguate, so name them here.
    foreign = sorted({
        n.split("-")[2] for n in names
        if n.count("-") >= 3 and len(n.split("-")[2]) == 7 and n.split("-")[2] != sha
    })
    if foreign:
        print(f"  {YELLOW}!{NC} assets from other commits: {', '.join(foreign)} (tag is {sha})")

    print(f"\n{BOLD}Node & wallet binaries{NC}")
    for platform, required in binary_platforms():
        # `tari_ootle` prefix only: the FFI zips embed the same version-sha-platform
        # tail and would otherwise stand in for a binary leg that never ran.
        present = [
            n for n in names
            if n.startswith("tari_ootle") and f"-{sha}-{platform}" in n and not n.endswith(".sha256")
        ]
        if present:
            print(f"  {mark(OK)} {platform:<16} {len(present)} artifact(s)")
        elif required:
            print(f"  {mark(MISSING)} {platform:<16} missing — the docs download table will have no entry")
            blockers += 1
        else:
            print(f"  {mark(OPTIONAL)} {platform:<16} missing (best-effort leg)")

    print(f"\n{BOLD}FFI libs (ootle-go consumes these){NC}")
    for platform in FFI_PLATFORMS:
        asset = f"ootle_sdk_ffi_c-{tag[1:]}-{sha}-{platform}.zip"
        if asset in names:
            print(f"  {mark(OK)} {platform}")
        else:
            print(f"  {mark(MISSING)} {platform} — {asset}")
            blockers += 1

    return blockers


def report_workflows(tag: str) -> None:
    print(f"\n{BOLD}Workflow runs for {tag}{NC}")
    runs = gh_json(
        "run", "list", "--repo", GH_REPO, "--branch", tag, "--limit", "30",
        "--json", "name,status,conclusion,url",
    )
    if not runs:
        print(f"  {YELLOW}!{NC} no runs found for this ref")
        return
    seen = {}
    for run in runs:  # newest first; keep the newest run per workflow
        seen.setdefault(run["name"], run)
    for name, run in sorted(seen.items()):
        if run["status"] != "completed":
            print(f"  {YELLOW}…{NC} {name:<36} {run['status']}")
        elif run["conclusion"] == "success":
            print(f"  {mark(OK)} {name:<36} success")
        else:
            print(f"  {mark(MISSING)} {name:<36} {run['conclusion']}  {run['url']}")


def report_npm(tag: str) -> None:
    print(f"\n{BOLD}npm{NC}")
    wanted = {}
    for directory, name in NPM_PACKAGES:
        manifest = json.loads(sh("git", "show", f"{tag}:{directory}/package.json"))
        wanted[name] = manifest["version"]
    with ThreadPoolExecutor(max_workers=len(wanted)) as pool:
        live = dict(zip(wanted, pool.map(npm_versions, wanted)))
    for name, version in wanted.items():
        versions = live[name]
        if versions is None:
            print(f"  {mark(OPTIONAL)} {name}@{version} — registry lookup failed")
        elif version in versions:
            print(f"  {mark(OK)} {name}@{version}")
        else:
            print(f"  {mark(MISSING)} {name}@{version} not on npm (the tag's publish job skips an unbumped version)")


def report_crates(tag: str) -> None:
    # Versions come from the working tree, since cargo metadata cannot read a tag:
    # accurate while the checkout is still on the release commit, which is where
    # this is run. render() says so when HEAD has moved on.
    print(f"\n{BOLD}crates.io{NC}")
    versions = local_versions()
    with ThreadPoolExecutor(max_workers=8) as pool:
        published = dict(zip(versions, pool.map(published_versions, versions)))
    unpublished = [
        name for name, _, _ in CRATES
        if published[name] is not None and not any(v == versions[name] for v, _ in published[name])
    ]
    if not unpublished:
        print(f"  {mark(OK)} every publish-set crate version is on crates.io")
        return
    print(f"  {mark(OPTIONAL)} {len(unpublished)} crate(s) pending: {', '.join(unpublished)}")
    print("    Run: ./scripts/publish_crates.py --execute")


def gh_file(repo: str, path: str) -> str:
    """A file's contents from another repo's default branch, or '' if unreadable."""
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/contents/{path}", "--jq", ".content"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        return ""
    try:
        return base64.b64decode(result.stdout).decode()
    except (ValueError, UnicodeDecodeError):
        return ""


def report_downstream(tag: str) -> None:
    """What each host SDK currently carries, against what this release ships.

    These repos are updated by hand after the release, so a line here is either a
    reminder or proof that the reminder was acted on.
    """
    version = tag[1:]
    print(f"\n{BOLD}Downstream SDKs{NC}")

    provenance = gh_file("tari-project/ootle-go", "internal/cffi/lib/PROVENANCE.md")
    carried = {row.split("|")[2].strip() for row in provenance.splitlines() if row.startswith("| ")
               and row.split("|")[2].strip()[:1].isdigit()}
    if not provenance:
        print(f"  {mark(OPTIONAL)} ootle-go   — could not read PROVENANCE.md")
    elif carried == {version}:
        print(f"  {mark(OK)} ootle-go   — vendors {version}")
    else:
        print(f"  {mark(MISSING)} ootle-go   — vendors {', '.join(sorted(carried)) or '?'}, needs {version}")
        print("      scripts/vendor_release.sh --tag {t} … per platform, then PR + tag".format(t=tag))

    catalog = gh_file("tari-project/ootle.ts", "pnpm-workspace.yaml")
    bindings_version = json.loads(sh("git", "show", f"{tag}:bindings/package.json"))["version"]
    for pin, want in (("@tari-project/ootle-ts-bindings", bindings_version),
                      ("@tari-project/ootle-wasm", version)):
        line = next((l for l in catalog.splitlines() if pin in l), "")
        have = line.split(":", 1)[1].strip().lstrip("^\"' ").rstrip("\"'") if line else ""
        if not catalog:
            print(f"  {mark(OPTIONAL)} ootle.ts   — could not read pnpm-workspace.yaml")
            break
        state = OK if have == want else MISSING
        print(f"  {mark(state)} ootle.ts   — {pin} pinned {have or '?'}, release has {want}")

    py = gh_file("tari-project/ootle-py", "src/ootle/_crypto/wasm/VERSION")
    upstream = next((l.split(":", 1)[1].strip() for l in py.splitlines() if l.startswith("upstream:")), "")
    if not py:
        print(f"  {mark(OPTIONAL)} ootle-py   — could not read the vendored wasm VERSION")
    elif upstream == version:
        print(f"  {mark(OK)} ootle-py   — vendors ootle-wasm {version}")
    else:
        print(f"  {mark(MISSING)} ootle-py   — vendors ootle-wasm {upstream or '?'}, release has {version}")
        print(f"      make update-wasm WASM_VERSION={version}, then PR + tag (PyPI publishes on the tag)")

    print(f"\n{BOLD}Deploy (manual){NC}")
    print("  · ready to deploy — confirm your nodes are on the new binary afterwards")
    print("  See checklists/release.md for the order and the gates between them.")


def render(tag: str) -> int:
    sha = sh("git", "rev-parse", "--short=7", f"{tag}^{{commit}}")
    print(f"{BOLD}Release status{NC} — {CYAN}{tag}{NC} ({sha})")
    if sh("git", "rev-parse", "--short=7", "HEAD") != sha:
        print(f"  {YELLOW}!{NC} HEAD is not {tag} — the crates.io section reads this checkout's versions")
    blockers = report_release(tag, sha)
    report_workflows(tag)
    report_npm(tag)
    report_crates(tag)
    report_downstream(tag)
    print()
    if blockers:
        print(f"{RED}{BOLD}DO NOT PUBLISH{NC} — {blockers} required artifact(s) missing")
    else:
        print(f"{GREEN}{BOLD}ARTIFACTS COMPLETE{NC} — safe to publish the draft once the testnet deploy is verified")
        print(f"  gh release edit {tag} --repo {GH_REPO} --draft=false")
    return blockers


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("tag", nargs="?", help="Release tag (default: the highest release tag).")
    p.add_argument("--watch", action="store_true", help="Re-render until no required artifact is missing.")
    p.add_argument("--interval", type=int, default=120, help="Seconds between renders with --watch.")
    args = p.parse_args()

    tag = args.tag or highest_release_tag()
    if not args.watch:
        return 1 if render(tag) else 0

    while True:
        print("\033[2J\033[H", end="")  # a watch redraws in place rather than scrolling
        if render(tag) == 0:
            return 0
        print(f"\n{CYAN}re-checking in {args.interval}s — ctrl-c to stop{NC}")
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
