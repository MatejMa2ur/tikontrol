# tikontrol container image: one static binary, no shell, no package manager, non-root.
# Multi-arch: linux/arm/v7, linux/arm64, linux/amd64 (and linux/arm/v5). Both build stages run on the
# build machine's own architecture and cross-compile with cargo-zigbuild, so no QEMU is needed.
#   docker buildx build --platform linux/arm64 -f Containerfile -t tikontrol:arm64 --output type=docker .
# RouterOS needs one tarball per architecture: see docs/DEPLOY.md.

FROM --platform=$BUILDPLATFORM node:22-alpine AS ui
WORKDIR /ui
COPY src/ui/package.json src/ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY src/ui/ ./
# snapshot.json holds real router data and is never part of a build (vite.config.ts stubs it anyway)
RUN rm -f src/lib/snapshot.json && npm run build

FROM --platform=$BUILDPLATFORM rust:1-alpine AS server
ARG TARGETPLATFORM
RUN apk add --no-cache musl-dev zig && cargo install --locked cargo-zigbuild
WORKDIR /src
COPY src/server/ ./
COPY --from=ui /ui/dist /ui/dist
ENV TIK_UI_DIST=/ui/dist
RUN case "$TARGETPLATFORM" in \
      linux/arm64) T=aarch64-unknown-linux-musl ;; \
      linux/arm/v7) T=armv7-unknown-linux-musleabihf ;; \
      linux/arm/v5) T=armv5te-unknown-linux-musleabi ;; \
      *) T=x86_64-unknown-linux-musl ;; \
    esac && rustup target add $T && cargo zigbuild --release --locked --target $T \
    && cp target/$T/release/tikontrol /tikontrol \
    && mkdir -p /data && chown 65532:65532 /data

FROM scratch
COPY --from=server /tikontrol /tikontrol
COPY --from=server --chown=65532:65532 /data /data
USER 65532:65532
ENV TIK_DATA=/data TIK_LISTEN=0.0.0.0:8443
EXPOSE 8443
VOLUME /data
ENTRYPOINT ["/tikontrol"]
