# Docker Build Notes

Create a folder ```sources``` and build a docker image.

```bash
mkdir sources
cd sources
git clone https://github.com/tari-project/tari.git
git clone https://github.com/tari-project/tari-ootle.git ootle
git clone https://github.com/tari-project/tari-connector.git
cp -v ootle/applications/tari_swarm_daemon/docker/cross-compile-aarch64.sh .
cd ..
docker build -f sources/tari-ootle/applications/tari_swarm_daemon/docker/tari_swarm.Dockerfile \ 
  -t local/tari-swarm .
```

# Targeted testing and cross platform builds

```bash
docker build -f tari_swarm/docker/tari_swarm.Dockerfile \
  -t local/tari-ootle-swarm --target=builder-tari .
```

or

```bash
docker build -f tari_swarm/docker/tari_swarm.Dockerfile \
  -t local/tari-ootle-swarm-arm64 --target=builder-tari \
  --platform linux/arm64 .
```

# Docker Testing Notes

Launching the docker image with local ports redirected to docker container ports 18000 to 19000

```bash
docker run --rm -it -p 18000-19000:18000-19000 \
  quay.io/tarilabs/tari-swarm
```

Using the folder ```sources```, builds can be done with
the docker image.

```bash
docker run --rm -it -p 18000-19000:18000-19000 \
  -v $PWD/sources/:/home/tari/sources-build \
  quay.io/tarilabs/tari-swarm:development_20230704_790dbea \
  /bin/bash
```
