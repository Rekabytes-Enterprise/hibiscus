# Official Rust builder snapshot, including its Rust/cargo-fuzz/LLVM tooling.
FROM gcr.io/oss-fuzz-base/base-builder-rust@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
COPY . $SRC/hibiscus
COPY .clusterfuzzlite/build.sh $SRC/build.sh
WORKDIR $SRC/hibiscus
