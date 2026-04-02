FROM rust:1.85-slim AS builder

WORKDIR /app

# 先複製 Cargo 設定，利用 Docker layer 快取相依性編譯
COPY Cargo.toml ./
# 建立假的 main.rs 讓 cargo 先下載並編譯相依套件
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# 複製實際程式碼並編譯
COPY src ./src
# 更新時間戳讓 cargo 重新編譯
RUN touch src/main.rs
RUN cargo build --release

# ── 執行階段（最小映像）──────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/stock-analysis-rust .

RUN mkdir -p /app/users && \
    useradd -m appuser && \
    chown -R appuser /app
USER appuser

CMD ["./stock-analysis-rust"]
