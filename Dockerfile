FROM public.ecr.aws/docker/library/rust:1.80.1-slim AS builder
WORKDIR /app
COPY . /app
RUN cargo fetch
RUN cargo build --release

FROM public.ecr.aws/docker/library/debian:stable-slim
COPY --from=builder /app/target/release/advoid /
ENTRYPOINT ["/advoid"]

