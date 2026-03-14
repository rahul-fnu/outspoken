# Outspoken - Multi-stage Linux build
# Usage:
#   docker build .                              # CPU-only build
#   docker build --build-arg CUDA=ON .          # With CUDA support
#   docker build --output type=local,dest=out . # Extract artifacts to ./out/

ARG CUDA=OFF

# ==============================================================================
# Stage 1: Build environment
# ==============================================================================
FROM rust:1.82-bookworm AS builder

ARG CUDA

# System dependencies for Tauri, cpal (ALSA), whisper.cpp, and bundling
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    pkg-config \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libasound2-dev \
    libssl-dev \
    curl \
    file \
    dpkg \
    dpkg-dev \
    libfuse2 \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 20 LTS
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Optional: CUDA toolkit for GPU-accelerated whisper
RUN if [ "$CUDA" = "ON" ]; then \
        apt-get update && apt-get install -y --no-install-recommends \
            nvidia-cuda-toolkit \
        && rm -rf /var/lib/apt/lists/*; \
    fi

WORKDIR /app

# -- Cache cargo dependencies --
# Copy only Cargo.toml first so deps are cached unless Cargo.toml changes
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* src-tauri/
RUN mkdir -p src-tauri/src \
    && echo 'fn main() {}' > src-tauri/src/main.rs \
    && echo '' > src-tauri/src/lib.rs \
    && cd src-tauri && cargo build --release || true \
    && rm -rf src-tauri/src

# -- Cache node_modules --
COPY package.json package-lock.json* ./
RUN npm ci 2>/dev/null || npm install

# -- Copy full source --
COPY . .

# Rebuild with real source (cached deps make this fast)
RUN npm run build

# Build Tauri app — produces .deb and .AppImage in target/release/bundle/
RUN cd src-tauri \
    && cargo install tauri-cli --version "^2" --locked \
    && cargo tauri build

# Collect artifacts into a single directory
RUN mkdir -p /output \
    && cp src-tauri/target/release/outspoken /output/ || true \
    && cp src-tauri/target/release/bundle/deb/*.deb /output/ || true \
    && cp src-tauri/target/release/bundle/appimage/*.AppImage /output/ || true

# ==============================================================================
# Stage 2: Minimal runtime
# ==============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    librsvg2-2 \
    libasound2 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /artifacts

# Copy build artifacts from builder
COPY --from=builder /output/ ./

# Install binary for direct execution
RUN cp outspoken /usr/local/bin/outspoken 2>/dev/null || true

CMD ["ls", "-la", "/artifacts"]
