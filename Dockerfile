FROM rust as planner
WORKDIR rs3
# We only pay the installation cost once,
# it will be cached from the second build onwards
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM rust as cacher
WORKDIR rs3
RUN cargo install cargo-chef
COPY --from=planner /rs3/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM rust:1.53 as build
ENV PKG_CONFIG_ALLOW_CROSS=1

WORKDIR rs3
COPY . .
COPY --from=cacher /rs3/target target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
RUN cargo install --path .

FROM gcr.io/distroless/cc-debian10

COPY --from=build /usr/local/cargo/bin/rs3 /usr/local/bin/rs3

EXPOSE 8080

ENV SERVER.HOST=0.0.0.0
ENV SERVER.PORT=8080

ENTRYPOINT ["/usr/local/bin/rs3"]