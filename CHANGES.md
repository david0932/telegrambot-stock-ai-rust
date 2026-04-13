# 修改記錄

## 2026-04-13

### feat: 支援 Gemini / Groq AI 切換

**背景：** 伺服器（Scloud Pte Ltd，台灣）被 Google Gemini API 封鎖，回傳 `User location is not supported for the API use.`

**修改內容：**

新增 `ai_provider` 欄位，可在 `config.json` 切換 AI 服務，無需重新編譯：

```json
{
  "ai_provider": "groq",
  "gemini_api_key": "...",
  "gemini_model": "gemini-2.0-flash",
  "groq_api_key": "...",
  "groq_model": "llama-3.3-70b-versatile"
}
```

- `"gemini"` → Google Gemini REST API（`/v1beta/models/{model}:generateContent`）
- `"groq"` → Groq OpenAI 相容 API（`/openai/v1/chat/completions`）

| 檔案 | 變更類型 |
|---|---|
| `src/analyzer.rs` | 新功能 — 雙 provider 支援，`call_gemini` / `call_groq` 分離 |
| `src/config.rs` | 新增 `ai_provider`、`groq_api_key`、`groq_model` 欄位 |
| `src/bot.rs` | 依 `ai_provider` 選取對應 key / model |
| `config.json.example` | 更新範例格式 |

---

### fix: 開盤價 fallback 到 indicators 第一筆資料

**問題：** 部分個股（尤其上櫃）Yahoo Finance 的 `regularMarketOpen` 回傳 `null`，導致開盤價顯示 `0.00`。

**修改：** `fetch_quote` 在 `regular_market_open` 為 null 時，改從 `indicators.quote[0].open[0]` 取第一筆值。

| 檔案 | 變更類型 |
|---|---|
| `src/fetcher.rs` | Bug fix — 開盤價 fallback 邏輯 |

---

## fix: 修正定時推播時區錯誤與休市仍推播的問題

### 問題描述

1. **時區錯誤**：推播時間偏移 8 小時
   - 預期在台灣時間 09:30 推播，實際卻在 17:30 才推播
   - 預期最後一則 13:30，實際延到 21:30

2. **休市仍繼續推播**：週末與盤後持續發送股價摘要

### 根本原因

**`tokio-cron-scheduler` v0.13 以 UTC 評估 cron 表達式**，不讀取系統 `TZ` 環境變數（`TZ=Asia/Taipei`）。

| config.json `key_times` | cron 實際解讀 | 台灣時間觸發 |
|---|---|---|
| 09:30 | 09:30 UTC | 17:30 ❌ |
| 10:30 | 10:30 UTC | 18:30 ❌ |
| 12:00 | 12:00 UTC | 20:00 ❌ |
| 13:25 | 13:25 UTC | 21:25 ❌ |
| 13:30 | 13:30 UTC | 21:30 ❌ |

此外，間隔推播（interval job）每 N 分鐘執行一次，24 小時全天無停，休市時也不例外。

### 修改內容

#### `src/scheduler.rs`

將 `key_times`（config 中儲存為台灣時間 UTC+8）在寫入 cron 表達式前，
先換算為 UTC（減 8 小時，modulo 24）：

```rust
// 台灣時間 UTC+8 → UTC（減 8 小時，modulo 24）
let hour_utc = (hour_tw + 24 - 8) % 24;
let cron = format!("0 {} {} * * *", minute, hour_utc);
```

修正後觸發時間：

| config.json `key_times` | cron（UTC） | 台灣時間觸發 |
|---|---|---|
| 09:30 | 01:30 UTC | 09:30 ✅ |
| 10:30 | 02:30 UTC | 10:30 ✅ |
| 12:00 | 04:00 UTC | 12:00 ✅ |
| 13:25 | 05:25 UTC | 13:25 ✅ |
| 13:30 | 05:30 UTC | 13:30 ✅ |

#### `src/bot.rs`

新增 `is_taiwan_market_open()` 函式，在 `push_summary_broadcast` 開頭加入守衛：

```rust
fn is_taiwan_market_open() -> bool {
    // 週一至週五，09:00–13:35 UTC+8
    // 使用 chrono::FixedOffset，不需新增相依套件
}
```

觸發條件：
- 星期一 ～ 星期五
- 台灣時間 09:00 ～ 13:35

休市（週末、盤前、盤後）時直接 return，不發送任何推播。

### 影響範圍

| 檔案 | 變更類型 |
|---|---|
| `src/scheduler.rs` | Bug fix — key_time cron 換算為 UTC |
| `src/bot.rs` | Bug fix — 加入市場時間守衛 |

### 注意事項

- `config.json` 的 `key_times` 格式**不變**，仍填寫台灣時間（如 `"09:30"`）
- 台灣無夏令時間（DST），UTC+8 為固定偏移，轉換結果永久有效

---

## feat: 加入 TWSE 國定假日檢查與修正交易時間

### 問題描述

1. **未檢查國定假日**：週一至週五的例假日（春節、清明、端午等）仍會觸發推播
2. **交易時間上限錯誤**：台灣股市收盤為 13:00，程式設定為 13:35

### 修改內容

#### `src/fetcher.rs`

新增 TWSE 休市日查詢功能：

```
API：https://www.twse.com.tw/rwd/zh/holidaySchedule/holidaySchedule?response=json&year={民國年}
```

- `fetch_twse_holidays(year)` — 取得指定年份所有休市日，以年份為 key 快取於記憶體
- `is_twse_holiday(date)` — 判斷指定日期是否為休市日
- 民國年格式（`"114/01/01"`）自動轉換為西元 `NaiveDate`
- API 呼叫失敗時回傳空清單（寧可推播，不漏推）

#### `src/bot.rs`

`is_taiwan_market_open()` 改為 `async`，判斷流程更新為四層：

```
1. 週六／週日？         → 休市
2. TWSE 休市日清單？    → 休市（含國定假日、補假）
3. 非 09:00～13:00？   → 盤前或盤後
4. 以上全部通過        → 交易中，允許推播
```

交易時間上限從 `13:35` 修正為 `13:00`。

### 影響範圍

| 檔案 | 變更類型 |
|---|---|
| `src/fetcher.rs` | 新功能 — TWSE 休市日 API 查詢與快取 |
| `src/bot.rs` | Bug fix — 假日判斷、收盤時間修正 |
