FROM rust:alpine3.13 as build

RUN apk update

RUN apk add cmake build-base protoc

WORKDIR /usr/src/app

RUN cargo init && rm Cargo.toml

COPY Cargo.toml Cargo.toml
# builds deps
RUN cargo build --release

RUN rm -f target/release/deps/snowflake*

COPY . .
# builds actual application code
RUN cargo build --release 

# ------------------------------------------------------------------------------
# Final Stage
# ------------------------------------------------------------------------------

FROM alpine:latest

RUN apk --no-cache add curl

RUN addgroup -g 1000 app

RUN adduser -D -s /bin/sh -u 1000 -G app app

WORKDIR /home/app/bin/

COPY --from=build /usr/src/app/target/release/snowflake .

RUN chown app:app snowflake

USER app

HEALTHCHECK --interval=10s --retries=2 --start-period=5s CMD [ "curl", "-f", "http://localhost/37550", "||", "exit 1" ]

CMD ["./snowflake"]