apt-get install --no-install-recommends --assume-yes \
  apt-transport-https \
  ca-certificates \
  curl \
  gpg \
  pkg-config \
  libsqlite3-dev \
  git \
  cmake \
  dh-autoreconf \
  libc++-dev \
  libc++abi-dev \
  libprotobuf-dev \
  protobuf-compiler \
  libncurses5-dev \
  libncursesw5-dev \
  build-essential \
  libudev-dev \
  libhidapi-dev \
  zip

wget https://github.com/openssl/openssl/releases/download/openssl-3.0.8/openssl-3.0.8.tar.gz
tar xvf openssl-3.0.8.tar.gz
cd openssl-3.0*/
./config
make
make install
ldconfig
tee /etc/profile.d/openssl.sh<<EOF
export PATH=/usr/local/openssl/bin:\$PATH
export LD_LIBRARY_PATH=/usr/local/openssl/lib:\$LD_LIBRARY_PATH
EOF
source /etc/profile.d/openssl.sh
openssl version