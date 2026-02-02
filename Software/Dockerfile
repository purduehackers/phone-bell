# Build stage for Raspberry Pi 64-bit
FROM rust:latest AS builder

# Install cross-compilation tools for aarch64
RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    pkg-config \
    cmake \
    libssl-dev:arm64 \
    libasound2-dev:arm64 \
    && rm -rf /var/lib/apt/lists/*

# Add the Rust target
RUN rustup target add aarch64-unknown-linux-gnu

# Set up cross-compilation environment
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
ENV CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
ENV PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR /app

# Copy source
COPY . .

# Build for Raspberry Pi (no-default-features to skip druid/GUI)
RUN cargo build --release --target aarch64-unknown-linux-gnu --no-default-features --features real
RUN aarch64-linux-gnu-strip /app/target/aarch64-unknown-linux-gnu/release/phone-bell-software

# The binary will be at /app/target/aarch64-unknown-linux-gnu/release/phone-bell-software

FROM scratch
COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/phone-bell-software /phone-bell-software-arm64
ENTRYPOINT ["/phone-bell-software-arm64"]

# The binary will be at /phone-bell-software-arm64 in the output image
