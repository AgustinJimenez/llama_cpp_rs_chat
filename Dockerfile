# Multi-stage Docker build for LLaMA Chat App
FROM rust:1.82-bullseye AS backend-builder

# Install system dependencies needed for llama-cpp-2 and Tauri
RUN apt-get update && apt-get install -y \
    cmake \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    clang \
    libclang-dev \
    libgtk-3-dev \
    libglib2.0-dev \
    libcairo2-dev \
    libpango1.0-dev \
    libatk1.0-dev \
    libgdk-pixbuf-2.0-dev \
    libsoup2.4-dev \
    libjavascriptcoregtk-4.0-dev \
    libwebkit2gtk-4.0-dev \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory
WORKDIR /app

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create dummy source to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN echo 'pub fn hello() {}' > src/lib.rs

# Enable llama-cpp-2 for Docker build
RUN sed -i 's/# llama-cpp-2 = "0.1.122"/llama-cpp-2 = "0.1.122"/' Cargo.toml

# Build dependencies (cached layer)
RUN cargo build --release --bin llama_cpp_chat --features docker
RUN rm -rf src

# Copy actual source code
COPY src ./src
COPY build.rs ./
COPY tauri.conf.json ./

# Remove dummy binary and rebuild with real source
RUN rm ./target/release/deps/llama_cpp_chat*
RUN cargo build --release --bin llama_cpp_chat --features docker

# Frontend builder stage
FROM node:18-bullseye AS frontend-builder

WORKDIR /app

# Copy package files
COPY package.json bun.lockb ./

# Install bun
RUN npm install -g bun

# Install dependencies
RUN bun install

# Copy frontend source
COPY src/ ./src/
COPY public/ ./public/
COPY index.html ./
COPY tsconfig.json tsconfig.node.json ./
COPY vite.config.ts ./
COPY tailwind.config.js ./
COPY postcss.config.js ./

# Build frontend
RUN bun run build

# Runtime image
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    libgomp1 \
    libomp5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 appuser

# Set working directory
WORKDIR /app

# Copy the built backend binary
COPY --from=backend-builder /app/target/release/llama_cpp_chat ./

# Copy frontend dist
COPY --from=frontend-builder /app/dist ./dist

# Create directories for conversations and models
RUN mkdir -p /app/assets/conversations /app/models
RUN chown -R appuser:appuser /app

# Switch to app user
USER appuser

# Expose port for the application
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Default command
CMD ["./llama_cpp_chat"]