FROM rust AS builder

ARG TARGET_CRATE=phone-bell-software

RUN apt-get update
#RUN apk add soapy-sdr-dev --repository=https://dl-cdn.alpinelinux.org/alpine/edge/testing
RUN apt-get install -y build-essential clang libudev-dev software-properties-common libasound2-dev pkg-config cmake
# RUN npm install npm@latest -g && \
#     npm install n -g && \
#     n latest


#RUN ldd /usr/lib/libSoapySDR.so && sleep 30

ENV RUSTFLAGS=-Ctarget-feature=-crt-static

WORKDIR /usr/src/phone-bell

COPY ./assets .

# Copy utility crates
COPY Cargo.lock ./
RUN printf "[workspace]\nmembers=[\"$TARGET_CRATE\"]" > Cargo.toml

RUN USER=root cargo new --bin $TARGET_CRATE

# Build external libraries
WORKDIR /usr/src/phone-bell/$TARGET_CRATE
COPY Cargo.toml .
# Clear all path-based (local) packages
RUN sed --in-place '/path = "\.\./d' Cargo.toml
#RUN if [[ $TARGETARCH = "amd64" ]] ; then cargo build --target x86_64-unknown-linux-musl --release ; \
#    else cargo build --target aarch64-unknown-linux-musl --release ; fi
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/phone-bell/target \
    cargo build --release -F real --no-default-features

# Client shit
# RUN mkdir client
# COPY $TARGET_CRATE/client/*.* ./client/
# WORKDIR /usr/src/orbital/$TARGET_CRATE/client
# RUN if [ -e "package.json" ] ; then npm install ; fi

# Copy and build internal libraries
WORKDIR /usr/src/phone-bell

WORKDIR /usr/src/phone-bell/$TARGET_CRATE
COPY Cargo.toml ./Cargo.toml

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/phone-bell/target \
    cargo build --release -F real --no-default-features
RUN rm -r src

RUN --mount=type=cache,target=/usr/src/phone-bell/target \
    rm /usr/src/phone-bell/target/release/deps/phone_bell_software*

# Build executable
# Copy actual source files
COPY . .

RUN mkdir ../out
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/phone-bell/target \
    cargo build --release -F real --no-default-features && mv ../target/release/$TARGET_CRATE ../out/app

WORKDIR /usr/src/phone-bell/out
ENTRYPOINT ["./app"]
#ENTRYPOINT ["sleep", "infinity"]

#FROM alpine AS runner
#
#EXPOSE 80
#
#ARG APP=/usr/src
#
#RUN apk update
#RUN apk add soapy-sdr-dev --repository=https://dl-cdn.alpinelinux.org/alpine/edge/testing
#
#RUN apk add clang npm build-base
#
#COPY --from=builder /usr/src/orbital/out/ ${APP}/tmp
#
#WORKDIR ${APP}
#RUN mv tmp/app .
#RUN if [[ -d "tmp/dist" ]] ; then cp -r ./tmp/dist ./dist ; fi
#RUN rm -r tmp
#
#ENTRYPOINT ["./app"]
