# Dockerfile
FROM ubuntu:24.04 AS build

# Set the environment variable to the value of the build argument
ENV DEBIAN_FRONTEND=noninteractive

# Install necessary packages
RUN apt-get update && \
    apt-get -y install git rustc cargo cmake autoconf automake build-essential libfontconfig1-dev pkg-config ca-certificates

# Update CA certificates
RUN update-ca-certificates

# Clone the specific tag from the Git repository
WORKDIR /app
ADD . .

# Build the project
RUN cargo build --release

# Run tests (Currently not existing, only placeholder)
# RUN cargo test --release

# Verify that the build was successful
RUN if [ ! -f /app/target/release/sidelb ]; then exit 1; fi

# Create final, minimal image
FROM scratch

# Copy webdav_bandwidth_calc
COPY --from=build /app/target/release/sidelb /sidelb
