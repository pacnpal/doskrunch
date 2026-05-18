# doskrunch stub build image.
#
# Open Watcom v2 ships a self-contained Linux x86-64 build as a snapshot
# tarball on the project's GitHub releases (refreshed nightly off the
# 'Current-build' tag). We unpack it into /opt/watcom and set the standard
# WATCOM/PATH/INCLUDE/EDPATH environment variables. The previous handoff
# referenced volkertb/debian-open-watcom, but that image was removed from
# Docker Hub; sourcing the toolchain directly from upstream is the more
# robust path anyway.
#
# Build:    docker build -t doskrunch-watcom stubs/
# Use:      docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
FROM debian:bookworm-slim

ARG OW_URL=https://github.com/open-watcom/open-watcom-v2/releases/download/Current-build/ow-snapshot.tar.xz

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl xz-utils make \
 && mkdir -p /opt/watcom \
 && curl -fsSL "$OW_URL" | tar -xJ -C /opt/watcom \
 && apt-get purge -y curl xz-utils \
 && apt-get autoremove -y \
 && rm -rf /var/lib/apt/lists/*

ENV WATCOM=/opt/watcom \
    PATH=/opt/watcom/binl64:/opt/watcom/binl:$PATH \
    INCLUDE=/opt/watcom/h \
    EDPATH=/opt/watcom/eddat

WORKDIR /work
CMD ["make", "all"]
