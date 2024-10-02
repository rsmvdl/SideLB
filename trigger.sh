#!/bin/sh

# Exit immediately if a command exits with a non-zero status.
set -e

# Enable Docker BuildKit for efficient image building
export DOCKER_BUILDKIT=1

# Extract the version from Cargo.toml
SIDE_LB_VERSION=$(grep -m 1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')

# Check if SIDE_LB_VERSION is set correctly
if [ -z "$SIDE_LB_VERSION" ]; then
    echo "Error: Failed to extract SIDE_LB_VERSION from Cargo.toml."
    exit 1
fi

# Build the Docker image for the Rust project
echo "Building SideLB v. $SIDE_LB_VERSION"
docker build \
    --build-arg SIDE_LB_VERSION="$SIDE_LB_VERSION" \
    --tag=sidelb:static \
    --output type=local,dest=build .
