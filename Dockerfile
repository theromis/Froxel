# Froxel's MCP server, headless, from a release build. Glama and other MCP
# directories start this image and talk to it over stdin/stdout.
#
#   docker build -t froxel-mcp .
#   docker run -i --rm froxel-mcp
FROM debian:bookworm-slim

ARG FROXEL_VERSION=0.2.0
# Set by BuildKit; amd64 when the builder does not set it.
ARG TARGETARCH

RUN set -eu; \
    case "${TARGETARCH:-amd64}" in \
      amd64) arch=x86_64 ;; \
      arm64) arch=aarch64 ;; \
      *) echo "no Froxel build for ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl; \
    name="froxel-${FROXEL_VERSION}-linux-${arch}"; \
    base="https://github.com/theromis/Froxel/releases/download/v${FROXEL_VERSION}"; \
    cd /tmp; \
    curl -fsSLO "${base}/${name}.tar.gz"; \
    curl -fsSLO "${base}/SHA256SUMS.txt"; \
    grep " ${name}.tar.gz\$" SHA256SUMS.txt | sha256sum -c -; \
    tar -xzf "${name}.tar.gz" -C /opt; \
    ln -s "/opt/${name}/froxel" /usr/local/bin/froxel; \
    rm -f "${name}.tar.gz" SHA256SUMS.txt; \
    apt-get purge -y curl; \
    apt-get autoremove -y; \
    rm -rf /var/lib/apt/lists/*

ENTRYPOINT ["froxel", "mcp"]
