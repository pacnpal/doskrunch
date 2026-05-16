# doskrunch stub build image.
#
# volkertb/debian-open-watcom is a maintained Debian image with Open
# Watcom v2 installed and on PATH. We pin to a digest in CI; here we
# float on :latest so contributors get something usable out of the box.
#
# Build:    docker build -t doskrunch-watcom stubs/
# Use:      docker run --rm -v "$PWD/stubs:/work" -w /work doskrunch-watcom make all
FROM volkertb/debian-open-watcom:latest

# Keep the image lean — Watcom is already on PATH. We just need make.
RUN apt-get update \
 && apt-get install -y --no-install-recommends make \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /work
CMD ["make", "all"]
