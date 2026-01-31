#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "Building Docker image..."
docker build -t phone-bell-builder .

echo "Extracting binary..."
docker create --name phone-bell-temp phone-bell-builder
docker cp phone-bell-temp:/app/target/aarch64-unknown-linux-gnu/release/phone-bell-software ./phone-bell-software-arm64
docker rm phone-bell-temp

echo "Stripping debug symbols..."
docker run --rm -v "$(pwd):/out" phone-bell-builder \
  aarch64-linux-gnu-strip -o /out/phone-bell-software-arm64 /out/phone-bell-software-arm64

echo "Done! Binary: phone-bell-software-arm64"
ls -lh phone-bell-software-arm64
