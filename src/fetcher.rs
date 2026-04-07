// fetcher.rs - Yahoo Finance API 即時報價與歷史資料
//
// 對應 Python 的 src/fetcher.py
//
// Python 用 yfinance 套件，Rust 沒有對應套件，
// 需直接呼叫 Yahoo Finance 的非官方 REST API：
//   即時報價: https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=1d
//   歷史日K:  https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=1mo

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;

/// 單一股票即時報價
///
/// # 對應 Python
/// ```python
/// {
///     "symbol": "2330.TW",
///     "name": "台積電",
///     "price": 950.0,
///     ...
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub volume: i64,      // 單位：張（1張=1000股）
    pub change: f64,
    pub change_pct: f64,
}

/// 單日歷史K線
#[derive(Debug, Clone)]
pub struct DayBar {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
}

// Yahoo Finance API 回應的 JSON 結構（只取需要的欄位）
#[derive(Deserialize)]
struct YfResponse {
    chart: YfChart,
}

#[derive(Deserialize)]
struct YfChart {
    result: Option<Vec<YfResult>>,
}

#[derive(Deserialize)]
struct YfResult {
    meta: YfMeta,
    timestamp: Option<Vec<i64>>,
    indicators: Option<YfIndicators>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YfMeta {
    symbol: String,
    regular_market_price: Option<f64>,
    chart_previous_close: Option<f64>,
    previous_close: Option<f64>,
    regular_market_open: Option<f64>,
    regular_market_day_high: Option<f64>,
    regular_market_day_low: Option<f64>,
    regular_market_volume: Option<i64>,
    long_name: Option<String>,
    short_name: Option<String>,
}

#[derive(Deserialize)]
struct YfIndicators {
    quote: Option<Vec<YfQuoteData>>,
}

#[derive(Deserialize)]
struct YfQuoteData {
    open: Option<Vec<Option<f64>>>,
    high: Option<Vec<Option<f64>>>,
    low: Option<Vec<Option<f64>>>,
    close: Option<Vec<Option<f64>>>,
    volume: Option<Vec<Option<i64>>>,
}

// ── 證交所休市日 ──────────────────────────────────────────────────

static HOLIDAY_CACHE: OnceLock<RwLock<HashMap<i32, Vec<NaiveDate>>>> = OnceLock::new();

fn holiday_cache() -> &'static RwLock<HashMap<i32, Vec<NaiveDate>>> {
    HOLIDAY_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

#[derive(Deserialize)]
struct TwseHolidayResp {
    stat: String,
    data: Option<Vec<Vec<String>>>,
}

/// 從 TWSE 取得指定西元年的休市日清單，同一年份只呼叫一次（快取）
pub async fn fetch_twse_holidays(year: i32) -> Vec<NaiveDate> {
    {
        let cache = holiday_cache().read().await;
        if let Some(dates) = cache.get(&year) {
            return dates.clone();
        }
    }

    let roc_year = year - 1911;
    let url = format!(
        "https://www.twse.com.tw/rwd/zh/holidaySchedule/holidaySchedule?response=json&year={}",
        roc_year
    );

    let dates = match do_fetch_holidays(&url).await {
        Ok(d) => d,
        Err(e) => {
            log::warn!("無法取得 {year} 年 TWSE 休市日：{e}");
            vec![]
        }
    };

    holiday_cache().write().await.insert(year, dates.clone());
    dates
}

async fn do_fetch_holidays(url: &str) -> Result<Vec<NaiveDate>> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?;

    let body: TwseHolidayResp = resp.json().await?;
    if body.stat != "OK" {
        return Ok(vec![]);
    }

    let mut dates = vec![];
    for row in body.data.unwrap_or_default() {
        // row[0] 格式："114/01/01"（民國年/月/日）
        let Some(s) = row.first() else { continue };
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 3 { continue; }
        let (Ok(roc_y), Ok(m), Ok(d)) = (
            parts[0].parse::<i32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else { continue };
        if let Some(date) = NaiveDate::from_ymd_opt(roc_y + 1911, m, d) {
            dates.push(date);
        }
    }
    Ok(dates)
}

/// 判斷指定日期是否為 TWSE 休市日（含國定假日與補假）
pub async fn is_twse_holiday(date: NaiveDate) -> bool {
    fetch_twse_holidays(date.year()).await.contains(&date)
}

/// 抓取即時報價
///
/// # 對應 Python
/// ```python
/// def fetch_quote(symbol: str) -> dict | None:
///     ticker = yf.Ticker(symbol)
///     info = ticker.info
///     ...
/// ```
pub async fn fetch_quote(symbol: &str) -> Result<Option<Quote>> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d",
        symbol
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let data: YfResponse = resp.json().await?;
    let result = match data.chart.result.and_then(|mut r| r.pop()) {
        Some(r) => r,
        None => return Ok(None),
    };

    let meta = result.meta;
    let price = match meta.regular_market_price {
        Some(p) => p,
        None => return Ok(None),
    };
    let prev_close = match meta.chart_previous_close.or(meta.previous_close) {
        Some(p) => p,
        None => return Ok(None),
    };

    let change = price - prev_close;
    let change_pct = (change / prev_close) * 100.0;
    let name = meta.long_name
        .or(meta.short_name)
        .unwrap_or_else(|| symbol.replace(".TWO", "").replace(".TW", ""));

    Ok(Some(Quote {
        symbol: symbol.to_string(),
        name,
        price,
        open: meta.regular_market_open.unwrap_or(0.0),
        high: meta.regular_market_day_high.unwrap_or(0.0),
        low: meta.regular_market_day_low.unwrap_or(0.0),
        volume: meta.regular_market_volume.unwrap_or(0) / 1000,
        change: (change * 100.0).round() / 100.0,
        change_pct: (change_pct * 100.0).round() / 100.0,
    }))
}

/// 抓取近 N 個交易日的日K資料
///
/// # 對應 Python
/// ```python
/// def fetch_history(symbol: str, days: int = 10) -> list[dict] | None:
///     ticker = yf.Ticker(symbol)
///     df = ticker.history(period="1mo")
///     ...
/// ```
pub async fn fetch_history(symbol: &str, days: usize) -> Result<Option<Vec<DayBar>>> {
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1mo",
        symbol
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let data: YfResponse = resp.json().await?;
    let result = match data.chart.result.and_then(|mut r| r.pop()) {
        Some(r) => r,
        None => return Ok(None),
    };

    let timestamps = match result.timestamp {
        Some(t) => t,
        None => return Ok(None),
    };
    let indicators = match result.indicators {
        Some(i) => i,
        None => return Ok(None),
    };
    let quote_data = match indicators.quote.and_then(|mut q| q.pop()) {
        Some(q) => q,
        None => return Ok(None),
    };

    let opens = quote_data.open.unwrap_or_default();
    let highs = quote_data.high.unwrap_or_default();
    let lows = quote_data.low.unwrap_or_default();
    let closes = quote_data.close.unwrap_or_default();
    let volumes = quote_data.volume.unwrap_or_default();

    let mut bars: Vec<DayBar> = timestamps
        .iter()
        .enumerate()
        .filter_map(|(i, &ts)| {
            let date = chrono::DateTime::from_timestamp(ts, 0)?
                .format("%Y-%m-%d")
                .to_string();
            Some(DayBar {
                date,
                open: opens.get(i).and_then(|v| *v).unwrap_or(0.0),
                high: highs.get(i).and_then(|v| *v).unwrap_or(0.0),
                low: lows.get(i).and_then(|v| *v).unwrap_or(0.0),
                close: closes.get(i).and_then(|v| *v).unwrap_or(0.0),
                volume: volumes.get(i).and_then(|v| *v).unwrap_or(0) / 1000,
            })
        })
        .collect();

    // 取最近 N 筆
    if bars.len() > days {
        bars = bars.split_off(bars.len() - days);
    }

    Ok(Some(bars))
}
