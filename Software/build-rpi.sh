#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "Building Docker image..."
docker build -o . .
echo "Done! Binary: phone-bell-software-arm64"
ls -lh phone-bell-software-arm64
