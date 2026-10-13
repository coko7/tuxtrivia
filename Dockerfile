# Build the WASM bundle, then serve it as static files.
FROM rust:1-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
 && cargo install --locked trunk
WORKDIR /src
COPY . .
RUN cd crates/web && trunk build --release

FROM joseluisq/static-web-server:2
COPY --from=build /src/crates/web/dist /public
