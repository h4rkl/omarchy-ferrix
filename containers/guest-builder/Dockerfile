# syntax=docker/dockerfile:1
# Guest builder for the postmarketOS/pmbootstrap route (generic AArch64 QEMU
# `virt` only). Pinned inputs come from targets/qemu-aarch64/pmos/pins.env via
# --build-arg; the defaults below must match that file.
#
# Runtime requirements: pmbootstrap creates chroots, bind-mounts /proc, /sys
# and /dev, mounts loop devices (losetup/kpartx) and partitions disk images.
# That needs CAP_SYS_ADMIN, CAP_SYS_CHROOT, CAP_MKNOD, an unconfined
# seccomp/AppArmor profile, access to /dev/loop-control and to loop
# partition nodes that appear only after the container has started (there is
# no udev in the container, so the entrypoint mounts devtmpfs over /dev and
# the kernel creates them). Docker-compatible engines expose that combination
# through `--privileged`; the host script passes that single flag and nothing else
# (no host PID/network namespaces, no host mounts beyond the work volume,
# project and output directories). The entrypoint checks these prerequisites
# and fails with a machine-readable record.
ARG GUEST_BUILDER_BASE=alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc
FROM ${GUEST_BUILDER_BASE}
ARG PMBOOTSTRAP_REPO=https://gitlab.postmarketos.org/postmarketOS/pmbootstrap.git
ARG PMBOOTSTRAP_TAG=3.11.1
ARG PMBOOTSTRAP_COMMIT=130b89d3c391596a1de9c12997b228b1e8a1f692

# pmbootstrap host requirements (pmb.config.required_programs): git, kpartx,
# losetup (with --json, so util-linux rather than BusyBox), openssl, ps, tar,
# chroot, sh. sudo is how pmbootstrap escalates from its unprivileged user;
# su-exec drops the entrypoint from root to that user. python3 + setuptools
# install pmbootstrap from the pinned checkout with no network access to PyPI.
RUN apk add --no-cache \
        python3 py3-setuptools py3-pip \
        git openssl ca-certificates \
        util-linux multipath-tools procps-ng tar coreutils \
        sudo su-exec \
    && adduser -D -u 1000 pmos \
    && printf 'pmos ALL=(ALL) NOPASSWD: ALL\n' > /etc/sudoers.d/pmos \
    && chmod 0440 /etc/sudoers.d/pmos \
    && printf '[safe]\n\tdirectory = *\n' > /home/pmos/.gitconfig \
    && chown pmos:pmos /home/pmos/.gitconfig

RUN git clone --quiet --depth 1 --branch "${PMBOOTSTRAP_TAG}" "${PMBOOTSTRAP_REPO}" /opt/pmbootstrap-src \
    && actual="$(git -C /opt/pmbootstrap-src rev-parse HEAD)" \
    && if [ "$actual" != "${PMBOOTSTRAP_COMMIT}" ]; then \
         printf 'pmbootstrap tag %s resolved to %s, expected %s\n' "${PMBOOTSTRAP_TAG}" "$actual" "${PMBOOTSTRAP_COMMIT}" >&2; exit 1; \
       fi \
    && pip install --quiet --no-index --no-deps --no-build-isolation --break-system-packages /opt/pmbootstrap-src \
    && printf '%s\n' "${PMBOOTSTRAP_TAG}" > /opt/pmbootstrap-tag \
    && printf '%s\n' "${PMBOOTSTRAP_COMMIT}" > /opt/pmbootstrap-commit \
    && pmbootstrap --version

COPY entrypoint.sh /usr/local/bin/guest-builder-entrypoint
RUN chmod 0755 /usr/local/bin/guest-builder-entrypoint

# pmbootstrap work directory (chroots, apk cache, pmaports clone, images).
# pmos-build.sh mounts a named container volume here (a macOS bind mount may
# not hold chroots: host sharing can drop ownership, setuid bits and device
# nodes); on Linux hosts FERRIX_PMOS_WORK_BIND=1 binds
# ${FERRIX_CACHE_DIR:-.cache/ferrix}/pmbootstrap.
VOLUME ["/cache/pmbootstrap"]
ENV FERRIX_PMBOOTSTRAP_CACHE=/cache/pmbootstrap
ENTRYPOINT ["/usr/local/bin/guest-builder-entrypoint"]
CMD ["pmbootstrap", "--version"]
