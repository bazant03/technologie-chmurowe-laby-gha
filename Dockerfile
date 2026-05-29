#syntax=docker/dockerfile:1

# --------- ETAP 1 ------------------------
FROM rust AS build
ARG TARGETPLATFORM

RUN apt-get update && apt-get install -y --no-install-recommends musl-tools 

RUN useradd -u 10001 weather
USER weather

COPY --chown=weather:weather weather-app-rust /weather-app-rust
WORKDIR /weather-app-rust

RUN case $TARGETPLATFORM in \
          "linux/amd64")  TARGET_TRIPLE=x86_64-unknown-linux-musl && \
                              export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc ;; \
          "linux/arm64")  TARGET_TRIPLE=aarch64-unknown-linux-musl && \
                              export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc ;; \
          *)              echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
     esac && \
     rustup target add "$TARGET_TRIPLE" && \
     cargo build --release --target "$TARGET_TRIPLE" && \
     mkdir -p out && \
     cp "target/$TARGET_TRIPLE/release/weather-rs" out/weather-rs

# --------- ETAP 2 ------------------------
FROM scratch AS final

LABEL org.opencontainers.image.authors="Marek Ruszecki <s101505@pollub.edu.pl>"

USER 10001:10001

COPY --from=build \ 
     --chown=10001:10001 \
     /weather-app-rust/out/weather-rs \ 
     /weather-rs

COPY --from=build \ 
     --chown=10001:10001 \
     /etc/ssl/certs/ca-certificates.crt \ 
     /etc/ssl/certs/ca-certificates.crt

EXPOSE 80

HEALTHCHECK --interval=5s --timeout=3s --retries=3 CMD ["/weather-rs", "--healthcheck"]

ENTRYPOINT ["/weather-rs"]