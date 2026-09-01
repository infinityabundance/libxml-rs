# syntax=docker/dockerfile:1
#
# Dockerfile — Phase 12 DOCKER-SUBSTITUTION court image.
#
# FROM libxml-rs/oracle:2.15.3 (built by docker/Dockerfile.oracle: canonical
# upstream libxml2 2.15.3 + libxslt 1.1.45 + libexslt 0.8.25 in /usr/local).
#
# The court runs ENTIRELY inside this minimal container: the consumer is
# compiled against the container oracle, then the libxml-rs DSOs (mounted
# read-only at /candidate) are substituted at runtime and the output must be
# byte-identical. Nothing is installed on the host beyond docker itself.
#
# Usage (see court-runner.sh):
#   docker build -t libxml-rs/docker-substitution:1 \
#       -f courts/suites/phase12/docker-substitution/Dockerfile \
#       courts/suites/phase12/docker-substitution
#   docker run --rm -v $ARTIFACT:/candidate:ro -v $RECEIPT_DIR:/out \
#       libxml-rs/docker-substitution:1
FROM libxml-rs/oracle:2.15.3

ENV DEBIAN_FRONTEND=noninteractive

# pkg-config is needed for the .pc plane of the court (the oracle image
# installs the build toolchain; pkg-config comes from it).
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# probe sources + in-container runner
COPY substitution-consumer.c substitution-consumer2.c substitution-runner.sh /probe/
RUN chmod +x /probe/substitution-runner.sh && mkdir -p /work /out

# candidate DSOs: mount the artifact dir at /candidate (ro)
# receipts: mount a host dir at /out
CMD ["/probe/substitution-runner.sh"]
