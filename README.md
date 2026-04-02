# telegrambot-stock-ai-rust

台股看盤 Telegram Bot，以 Rust 撰寫，支援即時報價、AI 分析、價格警示與定時推播。

## 功能

- 即時股價查詢（Yahoo Finance）
- 上市／上櫃股票搜尋（TWSE OpenAPI）
- AI 分析（Google Gemini）
- 價格／漲跌幅／成交量警示
- 定時推播追蹤清單
- 白名單用戶管理

## 指令

| 指令 | 說明 |
|------|------|
| `/search 台積電` | 搜尋股票代號或名稱 |
| `/price 2330` | 查詢即時行情 |
| `/summary` | 推送所有追蹤股票摘要 |
| `/list` | 查看追蹤清單與警示條件 |
| `/add 2330` | 新增追蹤股票 |
| `/remove 2330` | 移除追蹤股票 |
| `/alert 2330 price_below 900.0` | 設定價格警示 |
| `/analyze [代號]` | AI 分析（可不帶參數） |
| `/interval on\|off` | 開關定時推播 |
| `/status` | 查看運作狀態 |
| `/help` | 顯示指令說明 |

警示條件類型：`price_below`, `price_above`, `change_pct_above`, `change_pct_below`, `volume_above`

### 管理員指令

| 指令 | 說明 |
|------|------|
| `/allow {chat_id}` | 核准用戶申請 |
| `/deny {chat_id}` | 拒絕用戶申請 |
| `/revoke {chat_id}` | 撤銷授權 |
| `/users` | 列出所有授權用戶 |

## 部署

### 設定檔

建立 `config.json`：

```json
{
  "telegram_token": "你的 Telegram Bot Token",
  "gemini_api_key": "你的 Gemini API Key",
  "gemini_model": "gemini-2.0-flash",
  "admin_ids": ["你的 Telegram Chat ID"],
  "allowed_users": [],
  "schedule": {
    "interval_minutes": 30,
    "key_times": ["09:30", "13:25"]
  }
}
```

### Docker Compose

```yaml
services:
  bot:
    image: ghcr.io/david0932/telegrambot-stock-ai-rust:latest
    restart: unless-stopped
    environment:
      - TZ=Asia/Taipei
      - RUST_LOG=info
    volumes:
      - ./config.json:/app/config.json:ro
      - ./users:/app/users
```

```bash
docker compose up -d
docker compose logs -f
```

### 拉取最新 image

```bash
docker compose pull && docker compose up -d
```

## 開發

```bash
# 本地編譯
cargo build --release

# 執行
RUST_LOG=info ./target/release/stock-analysis-rust
```

## 技術棧

- **語言**：Rust 2021 edition
- **Bot 框架**：teloxide 0.12
- **非同步**：tokio
- **資料來源**：Yahoo Finance v8 API、TWSE OpenAPI
- **AI**：Google Gemini API
- **HTML 解析**：scraper + encoding_rs（BIG5）
- **排程**：tokio-cron-scheduler
- **容器**：Docker（ghcr.io）/ GitHub Actions CI
