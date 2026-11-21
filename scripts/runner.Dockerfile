ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS common
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git libxkbcommon-dev pkg-config python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

FROM common AS test
RUN apt-get update && apt-get install -y --no-install-recommends \
    fonts-dejavu-core libvulkan1 mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

FROM common AS release
RUN apt-get update && apt-get install -y --no-install-recommends gh dpkg-dev \
    gcc-mingw-w64-x86-64-win32 nsis \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add --toolchain "$RUST_VERSION" x86_64-pc-windows-gnu
