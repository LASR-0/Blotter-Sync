# Blotter Sync: a headless Blotter that keeps every desktop's planner in sync.
#
#   docker build -t blotter-sync .
#   docker run -d --name blotter-sync -p 7458:7458 -v blotter-sync:/data \
#     --read-only blotter-sync
#   docker exec blotter-sync blotter-sync pair

FROM rust:1.98-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY migrations migrations
COPY src src
RUN cargo build --release --locked \
 && install -D -m 0755 target/release/blotter-sync /out/blotter-sync \
 && install -d -m 0700 -o 65532 -g 65532 /out/data

# Distroless: no shell or package manager, CA certificates for calendar and
# holiday fetches, and a non-root user (65532).
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/blotter-sync /usr/local/bin/blotter-sync
COPY --from=build --chown=65532:65532 /out/data /data
ENV BLOTTER_SYNC_DATA_DIR=/data \
    BLOTTER_SYNC_LISTEN=0.0.0.0:7458 \
    # SQLite's temporary files go beside the data, so the root filesystem
    # can be mounted read-only.
    SQLITE_TMPDIR=/data
USER nonroot
VOLUME ["/data"]
EXPOSE 7458
ENTRYPOINT ["/usr/local/bin/blotter-sync"]
CMD ["serve"]
