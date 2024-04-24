FROM ubuntu:23.04

COPY target/release/collector-client /collector-client
COPY target/release/collector-server /collector-server
COPY config_mlx.yaml /config_mlx.yaml
ENV RUST_LOG=info
CMD ["/collector-client", "-c", "/config_mlx.yaml"]