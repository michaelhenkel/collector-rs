mkdir stats || true

cat << EOF > stats/docker-compose.yaml
version: '3.8'
services:
  rocky:
    restart: always
    image: michaelhenkel/rocky
    container_name: rocky
    pid: "host"
    network_mode: "host"
    cap_add:
      - ALL
    environment:
    - RUST_LOG=info
    command:
      - ./rocky-server
      - -a=0.0.0.0
      - -p=7777
      - --device=mlx5_0
      - --driver=mlx
    devices:
      - /dev/infiniband/uverbs0
      - /dev/infiniband/rdma_cm
      - /dev/infiniband/issm0
      - /dev/infiniband/umad0

  collector:
    image: michaelhenkel/collector-rs
    container_name: collector
    restart: always
    pid: "host"
    network_mode: "host"
    cap_add:
      - ALL
    environment:
    - RUST_LOG=info
    volumes:
      - ./config_mlx.yaml:/config_mlx.yaml
EOF

cat << 'EOF' > stats/config_mlx.yaml
address: 10.87.64.31:50055
namespace: "mlx"
counters:
- labels:
    "device": "mlx5_0"
  paths:
  - "/sys/class/infiniband/mlx5_0/ports/1/hw_counters"
  - "/sys/class/infiniband/mlx5_0/ports/1/counters"
  rate_keys:
  - "port_rcv_data"
  - "port_xmit_data"
interval: 1000
EOF

cd stats && docker-compose up -d