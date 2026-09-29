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

# The physical product is linked for Alpine/postmarketOS musl userspace.  The
# Rust udev/libinput/Smithay crates need the target-side development files at
# build time, while libudev-sys also probes a host library from its build
# script.  Keep both sides explicit: Debian's libudev-dev is host-only and
# these aarch64 APKs are the target sysroot inputs.  They are extracted as
# archives rather than installed or executed, so no foreign-architecture
# package scripts run in the x86_64 builder.
ARG ALPINE_VERSION=3.24
ARG ALPINE_EUDEV_DEV_VERSION=3.2.14-r6
ARG ALPINE_EUDEV_DEV_SHA256=39e5d3c9adfef77cf7c40b71a0cc16ab2358923404cf6c1832693f3b00df15b7
ARG ALPINE_EUDEV_LIBS_VERSION=3.2.14-r6
ARG ALPINE_EUDEV_LIBS_SHA256=330022dcf23de371d8df9b08f9bf9c9e98ced062b7352b518ee977dcaf977956
ARG ALPINE_LIBSEAT_DEV_VERSION=0.9.3-r0
ARG ALPINE_LIBSEAT_DEV_SHA256=b7bf7485a3f0d2f48f84609e96a99b70e51cc629daa1fd4b22e722a4ca2aa105
ARG ALPINE_LIBSEAT_VERSION=0.9.3-r0
ARG ALPINE_LIBSEAT_SHA256=f8875ed580a6525ad8faf0cec95f4aa70f1388f08a9eb43bae2d905be67e2d31
ARG ALPINE_ELOGIND_DEV_VERSION=255.24-r0
ARG ALPINE_ELOGIND_DEV_SHA256=f0fcf8e0d68b86ac5f1ae2556e8182fa0b52a66114e1c7631da3521c215f23c9
ARG ALPINE_LIBELOGIND_VERSION=255.24-r0
ARG ALPINE_LIBELOGIND_SHA256=11627ee58762791a7fe97eb71019c7cd8a318dee2ef7de7555dbe3319413aca3
ARG ALPINE_LIBDRM_DEV_VERSION=2.4.134-r0
ARG ALPINE_LIBDRM_DEV_SHA256=3e34b0a275c7b42f3b5b2887ca8fb1c656294ed40b01165a9735af9c7bf138e2
ARG ALPINE_LIBDRM_VERSION=2.4.134-r0
ARG ALPINE_LIBDRM_SHA256=ad796caa2b8685d8a3d586bc06bb3b710efb497e8503372ac65c93ab7260623b
ARG ALPINE_LIBXKBCOMMON_DEV_VERSION=1.13.1-r0
ARG ALPINE_LIBXKBCOMMON_DEV_SHA256=0a3542cb9a86e6788e44bd427b88ec92a6f3b1d3634da4724bfc764bbf70cc3c
ARG ALPINE_LIBXKBCOMMON_VERSION=1.13.1-r0
ARG ALPINE_LIBXKBCOMMON_SHA256=484370ec7cb91bd8035a7f947620885557daf0d4b10d8dbe50f207875828988e
ARG ALPINE_MESA_DEV_VERSION=26.1.6-r0
ARG ALPINE_MESA_DEV_SHA256=d6b92af5fdad93988cf07cb62a10214bf191e3f5911b33c7dccb9e96397babbc
ARG ALPINE_MESA_GBM_VERSION=26.1.6-r0
ARG ALPINE_MESA_GBM_SHA256=3a5487b688e3f072f134133b8a6538d552c9ebc86dedb5f0d54e2566f110f5da
ARG ALPINE_LIBINPUT_DEV_VERSION=1.31.3-r0
ARG ALPINE_LIBINPUT_DEV_SHA256=0f0fd646ab78b5bbdde09d85a5948d4d935fa87fbb5e13a993ff1478a942a10f
ARG ALPINE_LIBINPUT_LIBS_VERSION=1.31.3-r0
ARG ALPINE_LIBINPUT_LIBS_SHA256=9ce9a0d51f442ecee0ba370795c1dc6813115d3f271cbcb3ec12c9bd28732ab4
ARG ALPINE_PIXMAN_DEV_VERSION=0.46.4-r0
ARG ALPINE_PIXMAN_DEV_SHA256=c74cfa9a982fbf9a73d52cc3e88dab44273cef74334198f13ed1d54b1b0c7d51
ARG ALPINE_PIXMAN_VERSION=0.46.4-r0
ARG ALPINE_PIXMAN_SHA256=022ab7d2b766f3893e6c457a03c723fe3e79e9f009d8a62d06f1692d9f2fc77f

COPY zig-clang.py /usr/local/bin/ferrix-zig-clang.py
COPY zig-musl-link.py /usr/local/bin/ferrix-zig-musl-link.py

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
        libudev-dev \
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

# Install only the target development/runtime archives needed by the native
# Linux product compositor.  The package URLs and bytes are pinned above;
# extracting them under one sysroot keeps pkg-config from accidentally
# resolving the host x86_64 libraries for the aarch64-musl Cargo build.
RUN set -eux; \
    sysroot=/opt/ferrix/aarch64-linux-musl-sysroot; \
    mkdir -p "$sysroot" /tmp/alpine-pkgs; \
    fetch_apk() { \
      package=$1; \
      sha256=$2; \
      repo=$3; \
      url="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/${repo}/aarch64/${package}"; \
      curl --fail --location --retry 3 --connect-timeout 30 --output "/tmp/alpine-pkgs/${package}" "$url"; \
      printf '%s  %s\n' "$sha256" "/tmp/alpine-pkgs/${package}" | sha256sum --check --status; \
      tar --extract --gzip --file "/tmp/alpine-pkgs/${package}" --directory "$sysroot" \
        --no-same-owner --no-same-permissions --warning=no-unknown-keyword; \
    }; \
    fetch_apk "eudev-dev-${ALPINE_EUDEV_DEV_VERSION}.apk" "$ALPINE_EUDEV_DEV_SHA256" main; \
    fetch_apk "eudev-libs-${ALPINE_EUDEV_LIBS_VERSION}.apk" "$ALPINE_EUDEV_LIBS_SHA256" main; \
    fetch_apk "libseat-dev-${ALPINE_LIBSEAT_DEV_VERSION}.apk" "$ALPINE_LIBSEAT_DEV_SHA256" community; \
    fetch_apk "libseat-${ALPINE_LIBSEAT_VERSION}.apk" "$ALPINE_LIBSEAT_SHA256" community; \
    fetch_apk "elogind-dev-${ALPINE_ELOGIND_DEV_VERSION}.apk" "$ALPINE_ELOGIND_DEV_SHA256" community; \
    fetch_apk "libelogind-${ALPINE_LIBELOGIND_VERSION}.apk" "$ALPINE_LIBELOGIND_SHA256" community; \
    fetch_apk "libdrm-dev-${ALPINE_LIBDRM_DEV_VERSION}.apk" "$ALPINE_LIBDRM_DEV_SHA256" main; \
    fetch_apk "libdrm-${ALPINE_LIBDRM_VERSION}.apk" "$ALPINE_LIBDRM_SHA256" main; \
    fetch_apk "libxkbcommon-dev-${ALPINE_LIBXKBCOMMON_DEV_VERSION}.apk" "$ALPINE_LIBXKBCOMMON_DEV_SHA256" main; \
    fetch_apk "libxkbcommon-${ALPINE_LIBXKBCOMMON_VERSION}.apk" "$ALPINE_LIBXKBCOMMON_SHA256" main; \
    fetch_apk "mesa-dev-${ALPINE_MESA_DEV_VERSION}.apk" "$ALPINE_MESA_DEV_SHA256" main; \
    fetch_apk "mesa-gbm-${ALPINE_MESA_GBM_VERSION}.apk" "$ALPINE_MESA_GBM_SHA256" main; \
    fetch_apk "libinput-dev-${ALPINE_LIBINPUT_DEV_VERSION}.apk" "$ALPINE_LIBINPUT_DEV_SHA256" community; \
    fetch_apk "libinput-libs-${ALPINE_LIBINPUT_LIBS_VERSION}.apk" "$ALPINE_LIBINPUT_LIBS_SHA256" community; \
    fetch_apk "pixman-dev-${ALPINE_PIXMAN_DEV_VERSION}.apk" "$ALPINE_PIXMAN_DEV_SHA256" main; \
    fetch_apk "pixman-${ALPINE_PIXMAN_VERSION}.apk" "$ALPINE_PIXMAN_SHA256" main; \
    test -f "$sysroot/usr/include/libudev.h"; \
    test -f "$sysroot/usr/lib/pkgconfig/libudev.pc"; \
    test -f "$sysroot/usr/lib/pkgconfig/libseat.pc"; \
    test -f "$sysroot/usr/lib/pkgconfig/libelogind.pc"; \
    test -f "$sysroot/usr/lib/pkgconfig/libdrm.pc"; \
    test -f "$sysroot/usr/lib/pkgconfig/xkbcommon.pc"; \
    test -f "$sysroot/usr/lib/pkgconfig/gbm.pc"; \
    test -f "$sysroot/usr/lib/libudev.so"; \
    test -f "$sysroot/usr/lib/libseat.so"; \
    test -f "$sysroot/usr/lib/libdrm.so"; \
    test -f "$sysroot/usr/lib/libxkbcommon.so"; \
    test -f "$sysroot/usr/lib/libgbm.so"; \
    test -f "$sysroot/usr/lib/libinput.so"; \
    test -f "$sysroot/usr/lib/libpixman-1.so"; \
    rm -rf /tmp/alpine-pkgs

ENV PKG_CONFIG_PATH_aarch64_unknown_linux_musl=/opt/ferrix/aarch64-linux-musl-sysroot/usr/lib/pkgconfig \
    PKG_CONFIG_SYSROOT_DIR_aarch64_unknown_linux_musl=/opt/ferrix/aarch64-linux-musl-sysroot \
    PKG_CONFIG_ALLOW_CROSS_aarch64_unknown_linux_musl=1

# Zig supplies a deterministic aarch64-musl C linker without making the
# physical route depend on a host cross-toolchain.  The wrapper is named as
# required by dev/profile.env and is intentionally a linker-compatible command,
# not an architecture-emulation substitute: this image itself must run on a
# native x86_64 Linux Docker engine.
# Debian Bookworm's clang package is 14, but this kernel revision gates
# Kconfig on LLVM >= 17.  Zig carries a pinned LLVM 19 toolchain and can act
# as the clang/lld driver for both host tools and the arm64 Kbuild.  Keep the
# wrappers in PATH ahead of the Debian LLVM 14 tools. Zig 0.14 does not expose
# `objdump` or `strip` subcommands, and its `objcopy` rejects Kbuild's
# `--prefix-symbols`; use the pinned Debian LLVM binutils for those operations.
RUN curl --fail --location --retry 3 --connect-timeout 30 --output /tmp/zig.tar.xz "$ZIG_URL" \
    && printf '%s  %s\n' "$ZIG_SHA256" /tmp/zig.tar.xz | sha256sum --check --status \
    && tar -xJf /tmp/zig.tar.xz -C /opt \
    && mv "/opt/zig-x86_64-linux-$ZIG_VERSION" /opt/zig \
    && ln -s /opt/zig/zig /usr/local/bin/zig \
    && rm -f /tmp/zig.tar.xz \
    && printf '%s\n' '#!/bin/sh' 'exec python3 /usr/local/bin/ferrix-zig-clang.py "$@"' > /usr/local/bin/clang \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig c++ "$@"' > /usr/local/bin/clang++ \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig ld.lld "$@"' > /usr/local/bin/ld.lld \
    && printf '%s\n' '#!/bin/sh' 'exec /opt/zig/zig ar "$@"' > /usr/local/bin/llvm-ar \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/llvm-nm "$@"' > /usr/local/bin/llvm-nm \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/llvm-objcopy "$@"' > /usr/local/bin/llvm-objcopy \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/llvm-objdump "$@"' > /usr/local/bin/llvm-objdump \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/llvm-readelf "$@"' > /usr/local/bin/llvm-readelf \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/llvm-strip "$@"' > /usr/local/bin/llvm-strip \
    && chmod 0755 /usr/local/bin/clang /usr/local/bin/clang++ \
        /usr/local/bin/ld.lld /usr/local/bin/llvm-ar /usr/local/bin/llvm-nm \
        /usr/local/bin/llvm-objcopy /usr/local/bin/llvm-objdump \
        /usr/local/bin/llvm-readelf /usr/local/bin/llvm-strip \
    && printf '%s\n' '#!/bin/sh' 'exec python3 /usr/local/bin/ferrix-zig-musl-link.py "$@"' > /usr/local/bin/aarch64-linux-musl-gcc \
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
        "alpine=$ALPINE_VERSION" \
        "alpine-eudev-dev=$ALPINE_EUDEV_DEV_VERSION:$ALPINE_EUDEV_DEV_SHA256" \
        "alpine-eudev-libs=$ALPINE_EUDEV_LIBS_VERSION:$ALPINE_EUDEV_LIBS_SHA256" \
        "alpine-libseat-dev=$ALPINE_LIBSEAT_DEV_VERSION:$ALPINE_LIBSEAT_DEV_SHA256" \
        "alpine-libseat=$ALPINE_LIBSEAT_VERSION:$ALPINE_LIBSEAT_SHA256" \
        "alpine-elogind-dev=$ALPINE_ELOGIND_DEV_VERSION:$ALPINE_ELOGIND_DEV_SHA256" \
        "alpine-libelogind=$ALPINE_LIBELOGIND_VERSION:$ALPINE_LIBELOGIND_SHA256" \
        "alpine-libdrm-dev=$ALPINE_LIBDRM_DEV_VERSION:$ALPINE_LIBDRM_DEV_SHA256" \
        "alpine-libdrm=$ALPINE_LIBDRM_VERSION:$ALPINE_LIBDRM_SHA256" \
        "alpine-libxkbcommon-dev=$ALPINE_LIBXKBCOMMON_DEV_VERSION:$ALPINE_LIBXKBCOMMON_DEV_SHA256" \
        "alpine-libxkbcommon=$ALPINE_LIBXKBCOMMON_VERSION:$ALPINE_LIBXKBCOMMON_SHA256" \
        "alpine-mesa-dev=$ALPINE_MESA_DEV_VERSION:$ALPINE_MESA_DEV_SHA256" \
        "alpine-mesa-gbm=$ALPINE_MESA_GBM_VERSION:$ALPINE_MESA_GBM_SHA256" \
        "alpine-libinput-dev=$ALPINE_LIBINPUT_DEV_VERSION:$ALPINE_LIBINPUT_DEV_SHA256" \
        "alpine-libinput-libs=$ALPINE_LIBINPUT_LIBS_VERSION:$ALPINE_LIBINPUT_LIBS_SHA256" \
        "alpine-pixman-dev=$ALPINE_PIXMAN_DEV_VERSION:$ALPINE_PIXMAN_DEV_SHA256" \
        "alpine-pixman=$ALPINE_PIXMAN_VERSION:$ALPINE_PIXMAN_SHA256" \
      > /usr/share/doc/ferrix-pixel9a-builder/pins.txt \
    && rustup target add --toolchain 1.98.1 aarch64-unknown-linux-musl

COPY entrypoint.sh /usr/local/bin/ferrix-pixel9a-builder
RUN chmod 0755 /usr/local/bin/ferrix-pixel9a-builder

VOLUME ["/cache", "/out"]
ENTRYPOINT ["/usr/local/bin/ferrix-pixel9a-builder"]
CMD ["sh", "-c", "tools/pixel-9a/env.sh check build"]
