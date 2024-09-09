FROM public.ecr.aws/docker/library/rust:1.81.0-slim AS build

WORKDIR /app

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry \
    set -e; \
    cargo build --locked --release; \
    cp ./target/release/advoid /bin/advoid

FROM public.ecr.aws/docker/library/debian:stable-slim AS final
COPY --from=build /bin/advoid /bin/advoid
ENTRYPOINT ["/bin/advoid"]

