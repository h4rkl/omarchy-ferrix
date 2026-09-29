# syntax=docker/dockerfile:1
#
# Target-scoped native builder for the physical Google Pixel 9a route.
#
# This image is intentionally usable only as a linux/amd64 tool container on
# a native Linux x86_64 Docker engine.  The companion
# tools/pixel-9a/native-builder.sh enforces that boundary and never passes
# USB, an ADB server socket, or a host device node to this image.
#
# The base image, Rust channel, Zig linker, Android image tools, repo launcher,
# and pmbootstrap source are all pinned below.  Debian package versions are
# resolved from the digest-pinned Rust/Debian userspace at image-build time;
# source inputs which are not available as Debian packages are checked by
# commit or SHA-256 before installation.

ARG BUILDER_BASE=rust:1.98.1-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730
FROM ${BUILDER_BASE}

ARG ZIG_VERSION=0.14.1
ARG ZIG_SHA256=24aeeec8af16c381934a6cd7d95c807a8cb2cf7df9fa40d359aa884195c4716c
ARG ZIG_URL=https://ziglang.org/download/0.14.1/zig-x86_64-linux-0.14.1.tar.xz

ARG REPO_VERSION=2.54
ARG REPO_SHA256=6cba294d6218bbd4a1500598207b3979c752c7a122aef9429e4d7fef688833b5
ARG REPO_URL=https://storage.googleapis.com/git-repo-downloads/repo-2.54

ARG PMBOOTSTRAP_REPO=https://gitlab.postmarketos.org/postmarketOS/pmbootstrap.git
ARG PMBOOTSTRAP_COMMIT=130b89d3c391596a1de9c12997b228b1e8a1f692

ARG MKBOOTIMG_REPO=https://android.googlesource.com/platform/system/tools/mkbootimg
ARG MKBOOTIMG_COMMIT=ce21784168b932c3ad4fb539b3b65b4658f56356
ARG MKDTBOIMG_REPO=https://android.googlesource.com/platform/system/libufdt
ARG MKDTBOIMG_COMMIT=51befeeee29e8ea2cac77305451e9d781db2fd1a
ARG AVB_REPO=https://android.googlesource.com/platform/external/avb
ARG AVB_COMMIT=8201543ece1316d20f8a2002f70f8c076e28c7cb

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    HOME=/home/ferrix-builder \
    CARGO_HOME=/cache/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    ZIG_GLOBAL_CACHE_DIR=/cache/zig \
    ZIG_LOCAL_CACHE_DIR=/cache/zig-local \
    FERRIX_PIXEL9A_REPO_ROOT=/workspace \
    FERRIX_CACHE_DIR=/workspace/.cache/ferrix \
    FERRIX_OUT_DIR=/workspace/out

RUN apt-get update \
    && apt-get install --no-install-recommends --yes \
        bc \
        bison \
        build-essential \
        ca-certificates \
        clang \
        cpio \
        curl \
        e2fsprogs \
        file \
        flex \
        gawk \
        git \
        kmod \
        libelf-dev \
        libncurses-dev \
        libssl-dev \
        lld \
        llvm \
        make \
        m4 \
        pkg-config \
        python3 \
        python3-pip \
        python3-setuptools \
        rsync \
        sudo \
        tar \
        unzip \
        util-linux \
        xz-utils \
        zstd \
    && rm -rf /var/lib/apt/lists/*

# Zig supplies a deterministic aarch64-musl C linker without making the
# physical route depend on a host cross-toolchain.  The wrapper is named as
# required by dev/profile.env and is intentionally a linker-compatible command,
# not an architecture-emulation substitute: this image itself must run on a
# native x86_64 Linux Docker engine.
# Debian Bookworm's clang package is 14, but this kernel revision gates
# Kconfig on LLVM >= 17.  Zig carries a pinned LLVM 19 toolchain and can act
# as the clang/lld driver for both host tools and the arm64 Kbuild.  Keep the
# wrappers in PATH ahead of the Debian LLVM 14 tools.
RUN curl --fail --location --retry 3 --connect-timeout 30 --output /tmp/zig.tar.xz "$ZIG_URL" \
    && printf '%s  %s\n' "$ZIG_SHA256" /tmp/zig.tar.xz | sha256sum --check --status \
    && tar -xJf /tmp/zig.tar.xz -C /opt \
    && mv "/opt/zig-x86_64-linux-$ZIG_VERSION" /opt/zig \
    && ln -s /opt/zig/zig /usr/local/bin/zig \
    && rm -f /tmp/zig.tar.xz \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig cc "$@"' > /usr/local/bin/clang \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig c++ "$@"' > /usr/local/bin/clang++ \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig ld.lld "$@"' > /usr/local/bin/ld.lld \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig ar "$@"' > /usr/local/bin/llvm-ar \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig nm "$@"' > /usr/local/bin/llvm-nm \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig objcopy "$@"' > /usr/local/bin/llvm-objcopy \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig objdump "$@"' > /usr/local/bin/llvm-objdump \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig readelf "$@"' > /usr/local/bin/llvm-readelf \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig strip "$@"' > /usr/local/bin/llvm-strip \
    && chmod 0755 /usr/local/bin/clang /usr/local/bin/clang++ \
        /usr/local/bin/ld.lld /usr/local/bin/llvm-ar /usr/local/bin/llvm-nm \
        /usr/local/bin/llvm-objcopy /usr/local/bin/llvm-objdump \
        /usr/local/bin/llvm-readelf /usr/local/bin/llvm-strip \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig cc -target aarch64-linux-musl "$@"' > /usr/local/bin/aarch64-linux-musl-gcc \
    && chmod 0755 /usr/local/bin/aarch64-linux-musl-gcc

# The Android image tools are used only to compose source-built split images.
# Keep the exact upstream source revisions together under /opt and expose
# small wrappers so Python imports resolve from their checked-out directories.
RUN mkdir -p /opt/android/mkbootimg /opt/android/libufdt /opt/android/avb \
    && git clone --quiet --filter=blob:none --no-checkout "$MKBOOTIMG_REPO" /tmp/mkbootimg \
    && git -C /tmp/mkbootimg checkout --quiet "$MKBOOTIMG_COMMIT" \
    && test "$(git -C /tmp/mkbootimg rev-parse HEAD)" = "$(git -C /tmp/mkbootimg rev-parse "${MKBOOTIMG_COMMIT}^{commit}")" \
    && cp /tmp/mkbootimg/mkbootimg.py /opt/android/mkbootimg/mkbootimg.py \
    && cp -R /tmp/mkbootimg/gki /opt/android/mkbootimg/gki \
    && git clone --quiet --filter=blob:none --no-checkout "$MKDTBOIMG_REPO" /tmp/libufdt \
    && git -C /tmp/libufdt checkout --quiet "$MKDTBOIMG_COMMIT" \
    && test "$(git -C /tmp/libufdt rev-parse HEAD)" = "$(git -C /tmp/libufdt rev-parse "${MKDTBOIMG_COMMIT}^{commit}")" \
    && cp /tmp/libufdt/utils/src/mkdtboimg.py /opt/android/libufdt/mkdtboimg.py \
    && git clone --quiet --filter=blob:none --no-checkout "$AVB_REPO" /tmp/avb \
    && git -C /tmp/avb checkout --quiet "$AVB_COMMIT" \
    && test "$(git -C /tmp/avb rev-parse HEAD)" = "$(git -C /tmp/avb rev-parse "${AVB_COMMIT}^{commit}")" \
    && cp /tmp/avb/avbtool.py /opt/android/avb/avbtool.py \
    && printf '%s\n' '#!/bin/sh' 'exec python3 /opt/android/mkbootimg/mkbootimg.py "$@"' > /usr/local/bin/mkbootimg \
    && printf '%s\n' '#!/bin/sh' 'exec python3 /opt/android/libufdt/mkdtboimg.py "$@"' > /usr/local/bin/mkdtboimg.py \
    && printf '%s\n' '#!/bin/sh' 'exec python3 /opt/android/avb/avbtool.py "$@"' > /usr/local/bin/avbtool \
    && chmod 0755 /usr/local/bin/mkbootimg /usr/local/bin/mkdtboimg.py /usr/local/bin/avbtool \
    && rm -rf /tmp/mkbootimg /tmp/libufdt /tmp/avb

# repo is required by the optional Google Kleaf comparison workflow.  It is
# installed even though the selected radio-first Kbuild route does not invoke
# it, so env.sh check build describes one complete, stable build environment.
RUN curl --fail --location --retry 3 --connect-timeout 30 --output /usr/local/bin/repo "$REPO_URL" \
    && printf '%s  %s\n' "$REPO_SHA256" /usr/local/bin/repo | sha256sum --check --status \
    && chmod 0755 /usr/local/bin/repo \
    && printf '%s\n' "$REPO_VERSION" > /usr/local/share/ferrix-pixel9a-repo-version

# pmbootstrap has no third-party Python runtime dependency at the pinned
# release.  Install from its detached source checkout, with no PyPI access or
# unpinned dependency resolution.
RUN git clone --quiet --filter=blob:none --no-checkout "$PMBOOTSTRAP_REPO" /opt/pmbootstrap-src \
    && git -C /opt/pmbootstrap-src checkout --quiet "$PMBOOTSTRAP_COMMIT" \
    && test "$(git -C /opt/pmbootstrap-src rev-parse HEAD)" = "$PMBOOTSTRAP_COMMIT" \
    && python3 -m pip install --break-system-packages --no-cache-dir --no-index --no-deps --no-build-isolation /opt/pmbootstrap-src \
    && pmbootstrap --version

RUN useradd --create-home --uid 1000 --shell /bin/sh ferrix-builder \
    && printf '%s\n' 'ferrix-builder ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/ferrix-builder \
    && chmod 0440 /etc/sudoers.d/ferrix-builder \
    && mkdir -p /workspace /cache /cache/zig /cache/zig-local /out /usr/share/doc/ferrix-pixel9a-builder \
    && chown -R ferrix-builder:ferrix-builder /workspace /cache /out \
    && printf '%s\n' \
        "rust=1.98.1" \
        "zig=$ZIG_VERSION" \
        "pmbootstrap=$PMBOOTSTRAP_COMMIT" \
        "mkbootimg=$MKBOOTIMG_COMMIT" \
        "mkdtboimg=$MKDTBOIMG_COMMIT" \
        "avb=$AVB_COMMIT" \
        "repo=$REPO_VERSION" \
      > /usr/share/doc/ferrix-pixel9a-builder/pins.txt \
    && rustup target add --toolchain 1.98.1 aarch64-unknown-linux-musl

COPY entrypoint.sh /usr/local/bin/ferrix-pixel9a-builder
RUN chmod 0755 /usr/local/bin/ferrix-pixel9a-builder

VOLUME ["/cache", "/out"]
ENTRYPOINT ["/usr/local/bin/ferrix-pixel9a-builder"]
CMD ["sh", "-c", "tools/pixel-9a/env.sh check build"]
