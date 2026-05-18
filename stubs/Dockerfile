# doskrunch stub build image.
#
# Open Watcom v2 publishes monthly immutable releases (e.g.
# 2026-05-02-Build) alongside the rolling 'Current-build' tag. We pin
# the URL to a dated tag *and* verify a SHA256 of the snapshot so the
# build is bit-reproducible: the dated asset is never overwritten
# upstream, and the hash check fails loudly if something tampers with
# the download in transit. Bump both OW_URL and OW_SHA256 together
# when we explicitly want a newer toolchain.
#
# The previously-referenced volkertb/debian-open-watcom image was
# removed from Docker Hub, so we source the toolchain directly from
# upstream.
#
# Build:    docker build -t doskrunch-watcom stubs/
# Use:      docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
FROM debian:bookworm-slim

ARG OW_URL=https://github.com/open-watcom/open-watcom-v2/releases/download/2026-05-02-Build/ow-snapshot.tar.xz
ARG OW_SHA256=a339a23ab346b6e632f870aaa4c36a5424bb923197b96348e367ea07b84936cc

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
