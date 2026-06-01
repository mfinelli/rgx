FROM rust:slim AS source
WORKDIR /rgx
COPY . /rgx
RUN cargo vendor --locked

FROM source AS build
RUN cargo build --frozen --release --verbose

FROM build AS test
RUN cargo test

FROM debian:stable-slim

LABEL org.opencontainers.image.title=rgx
LABEL org.opencontainers.image.version=v0.1.0
LABEL org.opencontainers.image.description="command line regexp tester"
LABEL org.opencontainers.image.url=https://github.com/mfinelli/rgx
LABEL org.opencontainers.image.source=https://github.com/mfinelli/rgx
LABEL org.opencontainers.image.licenses=GPL-3.0-or-later

RUN useradd -r -U -m rgx
COPY --from=source /rgx /usr/src/rgx
COPY --from=build /rgx/target/release/rgx /usr/bin/rgx
USER rgx
CMD ["rgx"]
