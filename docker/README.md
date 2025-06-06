# Docker Build Notes

Create a folder ```sources``` and build a docker image.

```bash
mkdir -p sources
cd sources
git clone https://github.com/tari-project/tari-dan.git ootle
cd ootle
docker build -f docker/ootle.Dockerfile \
  -t local/ootle:testing .
```
