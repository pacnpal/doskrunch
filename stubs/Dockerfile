# doskrunch stub build image.
#
# Open Watcom v2 ships a self-contained Linux x86-64 build as a snapshot
# tarball on the project's GitHub releases. The 'Current-build' tag is
# the only published release, and its assets are overwritten in place —
# there are no semver-tagged binary releases to pin. To keep the stub
# blob bit-reproducible we pin OW_SHA256 to the snapshot we built
# against; the build aborts if the upstream asset is re-uploaded with
# different bytes. Bump both OW_URL and OW_SHA256 together when we
# explicitly want to upgrade the toolchain.
#
# The previously-referenced volkertb/debian-open-watcom image was
# removed from Docker Hub, so we source the toolchain directly from
# upstream.
#
# Build:    docker build -t doskrunch-watcom stubs/
# Use:      docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
FROM debian:bookworm-slim

ARG OW_URL=https://github.com/open-watcom/open-watcom-v2/releases/download/Current-build/ow-snapshot.tar.xz
ARG OW_SHA256=ec0bebd973ee339dc0425db184ab82a20e0fefee8be839ec75a366f47b12c86a

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl xz-utils make \
 && mkdir -p /opt/watcom \
 && curl -fsSL -o /tmp/ow.tar.xz "$OW_URL" \
 && echo "$OW_SHA256  /tmp/ow.tar.xz" | sha256sum -c - \
 && tar -xJ -C /opt/watcom -f /tmp/ow.tar.xz \
 && rm /tmp/ow.tar.xz \
 && apt-get purge -y curl xz-utils \
 && apt-get autoremove -y \
 && rm -rf /var/lib/apt/lists/*

ENV WATCOM=/opt/watcom \
    PATH=/opt/watcom/binl64:/opt/watcom/binl:$PATH \
    INCLUDE=/opt/watcom/h \
    EDPATH=/opt/watcom/eddat

WORKDIR /work
CMD ["make", "all"]
