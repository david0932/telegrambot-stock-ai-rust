# 修改記錄

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
