# Tari Ootle Docker image

This directory contains the Dockerfile that produces the `ghcr.io/tari-project/ootle`
container image. See [`docs/docker-build-pipeline.md`](../docs/docker-build-pipeline.md)
for the full architecture, CI workflow, and troubleshooting reference.

## Quick build

```bash
DOCKER_BUILDKIT=1 docker build \
  -f docker/ootle.Dockerfile \
  -t ootle:local .
```

The first build takes 15-25 minutes. Subsequent builds reuse the cargo-chef
and pnpm caches and are typically 30 seconds to 5 minutes.

## Quick run

The image ships the network node binaries, `tari_validator_node` and
`tari_indexer`, with no default `CMD`. Pick one when running:

```bash
docker run --rm ootle:local tari_validator_node --help
docker run --rm ootle:local tari_indexer --help
```

The wallet, swarm and operations tools are not in the image; use the release binaries.

The container runs as the non-root user `tari` (uid 1000), with `tini`
as PID 1 for signal forwarding and zombie reaping.

## Pulling from GHCR

```bash
docker pull ghcr.io/tari-project/ootle:latest
docker pull ghcr.io/tari-project/ootle:development
docker pull ghcr.io/tari-project/ootle:nightly
```

Released tags follow `vX.Y.Z` semver.
