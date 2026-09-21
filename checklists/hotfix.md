# Hotfix checklist

A fix on top of an already-released version, when `development` has moved on and you do not want
what it carries.

Shared reference: [README.md](README.md).

---

## 1. Branch and fix

- [ ] **Branch from the tag**, never from `development`:
      ```sh
      git checkout -b hotfix/v0.41.1 v0.41.0
      ```
- [ ] Apply the fix — cherry-pick from `development` where possible, so the two histories agree:
      ```sh
      git cherry-pick -x <sha>
      ```
- [ ] If the fix is not yet on `development`, write it here and plan the forward-port (step 5).

## 2. Version and changelog

- [ ] Bump the **patch** version: `[workspace.package].version` for a tier-3 fix, or the affected
      independent crate's own version.
      ```sh
      ./scripts/crate_versioning.py impact <crate>
      ```
      A non-breaking fix is a patch and dependents pick it up via `^0.y`. If the fix is breaking,
      it is not a hotfix — run [release-breaking.md](release-breaking.md) and cut a minor.
- [ ] Changelog entry for the patch version, naming what it fixes and who must act.
- [ ] ```sh
      ./scripts/release_check.py --since v0.41.0
      ```
      The baseline matters: against the previous *release*, the diff is the hotfix alone.

## 3. Land it

- [ ] Open the PR into `main` — a hotfix is a release-line change. `development` gets it by
      forward-port in step 5.
- [ ] CI green on `main`.

## 4. Release it

- [ ] Follow [release.md](release.md) from **step 2 (cut the tag)** onward: signed tag, watch the
      builds, deploy to the testnet, verify, publish the draft, publish crates, update the
      downstream SDKs.
- [ ] A hotfix earns the same watch-and-verify sequence. It is smaller, not safer.

## 5. Forward-port

- [ ] Merge `main` back into `development` (or cherry-pick the fix there) the **same day**, so the
      next release does not silently revert it.
- [ ] Confirm: `git log development --oneline | grep <fix-subject>` finds it.

## 6. Downstream

- [ ] Update the downstream SDKs only if the fix touches what they consume.
      `./scripts/release_status.py` tells you what is behind.
