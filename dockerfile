FROM lukemathwalker/cargo-chef:latest-rust-buster AS planner
USER root
WORKDIR /app
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef:latest-rust-buster AS builder
USER root
WORKDIR /app
RUN apt update
RUN apt install -y cmake
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin snowflake 

FROM gcr.io/distroless/cc AS runtime
#RUN addgroup -S myuser && adduser -S myuser -G myuser
COPY --from=builder /app/target/release/snowflake /app/
#USER myuser
CMD ["/app/snowflake"]