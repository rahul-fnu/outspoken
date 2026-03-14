# syntax=docker/dockerfile:1

# Multi-stage Dockerfile for building Outspoken on Linux
# Produces .deb and .AppImage artifacts

# Build arg for CUDA support
ARG CUDA_ENABLED=false

# ==============================================================================
# Stage 1: Build
# ==============================================================================
FROM rust:1.82-bookworm AS builder

ARG CUDA_ENABLED

# Install system dependencies for Tauri, cpal (ALSA), whisper.cpp, and bundling
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    libclang-dev \
    libasound2-dev \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    pkg-config \
    wget \
    file \
    dpkg \
    # AppImage tooling
    libfuse2 \
    # Node.js
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 20 LTS
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# -- Cache npm dependencies --
COPY package.json package-lock.json ./
RUN npm ci

# -- Cache cargo dependencies --
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/
COPY src-tauri/build.rs src-tauri/build.rs
# Create a dummy lib and main so cargo can resolve deps
RUN mkdir -p src-tauri/src \
    && echo "pub fn run() {}" > src-tauri/src/lib.rs \
    && echo "fn main() {}" > src-tauri/src/main.rs \
    && cd src-tauri \
    && if [ "$CUDA_ENABLED" = "true" ]; then \
         cargo build --release --features cuda; \
       else \
         cargo build --release; \
       fi \
    && rm -rf src/

# -- Copy full source --
COPY . .

# Build frontend
RUN npm run build

# Build Tauri app (produces .deb and .AppImage)
RUN cd src-tauri \
    && if [ "$CUDA_ENABLED" = "true" ]; then \
         cargo build --release --features cuda; \
       else \
         cargo build --release; \
       fi

# Use Tauri CLI to bundle
RUN npx tauri build \
      $(if [ "$CUDA_ENABLED" = "true" ]; then echo "-- --features cuda"; fi) \
    || true

# ==============================================================================
# Stage 2: Runtime / artifact extraction
# ==============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    libasound2 \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /artifacts

# Copy built artifacts
COPY --from=builder /app/src-tauri/target/release/bundle/deb/*.deb ./  2>/dev/null || true
COPY --from=builder /app/src-tauri/target/release/bundle/appimage/*.AppImage ./ 2>/dev/null || true
COPY --from=builder /app/src-tauri/target/release/outspoken ./outspoken 2>/dev/null || true

# Default: list artifacts
CMD ["ls", "-la", "/artifacts"]
