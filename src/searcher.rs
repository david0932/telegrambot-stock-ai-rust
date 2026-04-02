// searcher.rs - 台股清單（上市＋上櫃）與股票搜尋
//
// 對應 Python 的 src/searcher.py
//
// 上市：TWSE OpenAPI JSON
// 上櫃：TWSE ISIN 系統 HTML（MS950 編碼），用 scraper crate 解析

use anyhow::Result;
use scraper::{Html, Selector};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const TWSE_URL: &str = "https://openapi.twse.com.tw/v1/exchangeReport/STOCK_DAY_ALL";
const TPEX_URL: &str = "https://isin.twse.com.tw/isin/C_public.jsp?strMode=4";
const CACHE_TTL: Duration = Duration::from_secs(86400); // 24 小時

#[derive(Debug, Clone)]
pub struct Stock {
    pub code: String,
    pub name: String,
    pub market: Market,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Market {
    Listed, // 上市 .TW
    Otc,    // 上櫃 .TWO
}

impl Market {
    pub fn suffix(&self) -> &str {
        match self {
            Market::Listed => ".TW",
            Market::Otc => ".TWO",
        }
    }
}

/// 全域快取（對應 Python 的 module-level _cache）
struct Cache {
    stocks: Vec<Stock>,
    updated_at: Option<Instant>,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn get_cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        Mutex::new(Cache {
            stocks: vec![],
            updated_at: None,
        })
    })
}

/// 取得台股清單，結果快取 24 小時
///
/// # 對應 Python
/// ```python
/// def get_stock_list() -> list[dict]:
///     global _cache, _cache_time
///     if not _cache or time.time() - _cache_time > CACHE_TTL:
///         _cache = _fetch_stock_list()
///     return _cache
/// ```
pub async fn get_stock_list() -> Result<Vec<Stock>> {
    let mut cache = get_cache().lock().await;

    let expired = cache
        .updated_at
        .map(|t| t.elapsed() > CACHE_TTL)
        .unwrap_or(true);

    if expired {
        let mut stocks = vec![];

        match fetch_twse().await {
            Ok(list) => {
                log::info!("TWSE 股票清單載入：{} 檔", list.len());
                stocks.extend(list);
            }
            Err(e) => log::warn!("無法取得 TWSE 股票清單：{e}"),
        }

        match fetch_tpex().await {
            Ok(list) => {
                log::info!("TPEX 股票清單載入：{} 檔", list.len());
                stocks.extend(list);
            }
            Err(e) => log::warn!("無法取得 TPEX 股票清單：{e}"),
        }

        cache.stocks = stocks;
        cache.updated_at = Some(Instant::now());
    }

    Ok(cache.stocks.clone())
}

/// 依代號解析對應的 yfinance symbol（上市 .TW，上櫃 .TWO）
///
/// # 對應 Python
/// ```python
/// def resolve_symbol(code: str) -> str:
///     for stock in get_stock_list():
///         if stock["code"] == code:
///             return f"{code}.TWO" if stock["market"] == "上櫃" else f"{code}.TW"
///     return f"{code}.TW"
/// ```
pub async fn resolve_symbol(code: &str) -> String {
    if let Ok(stocks) = get_stock_list().await {
        if let Some(stock) = stocks.iter().find(|s| s.code == code) {
            return format!("{}{}", code, stock.market.suffix());
        }
    }
    format!("{}.TW", code) // 預設上市
}

/// 查詢股票中文名稱
pub async fn get_stock_name(code: &str) -> Option<String> {
    let stocks = get_stock_list().await.ok()?;
    stocks.iter().find(|s| s.code == code).map(|s| s.name.clone())
}

/// 依代號或名稱搜尋（部分比對）
pub async fn search_stocks(query: &str, limit: usize) -> Vec<Stock> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }
    let stocks = get_stock_list().await.unwrap_or_default();
    stocks
        .into_iter()
        .filter(|s| s.code.contains(query) || s.name.contains(query))
        .take(limit)
        .collect()
}

// ── 內部：抓取上市清單 ─────────────────────────────────────────

async fn fetch_twse() -> Result<Vec<Stock>> {
    // TWSE 回傳 JSON，格式：[{"Code": "2330", "Name": "台積電", ...}, ...]
    let client = reqwest::Client::new();
    let resp: Vec<serde_json::Value> = client
        .get(TWSE_URL)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .json()
        .await?;

    let stocks = resp
        .iter()
        .filter_map(|item| {
            let code = item["Code"].as_str()?.trim().to_string();
            let name = item["Name"].as_str()?.trim().to_string();
            if code.is_empty() || name.is_empty() {
                return None;
            }
            Some(Stock { code, name, market: Market::Listed })
        })
        .collect();

    Ok(stocks)
}

// ── 內部：抓取上櫃清單 ─────────────────────────────────────────

async fn fetch_tpex() -> Result<Vec<Stock>> {
    // TPEX 使用 TWSE ISIN 系統 HTML（MS950 編碼）
    // 每個 <td> 裡代號和名稱用全形空格（U+3000）分隔
    let client = reqwest::Client::new();
    let bytes = client
        .get(TPEX_URL)
        .header("User-Agent", "Mozilla/5.0")
        .header("Referer", "https://www.tpex.org.tw/")
        .send()
        .await?
        .bytes()
        .await?;

    // MS950 → UTF-8 解碼
    // encoding_rs 是 reqwest 的依賴，可直接使用
    let (text, _, _) = encoding_rs::WINDOWS_949.decode(&bytes);

    let document = Html::parse_document(&text);
    let td_selector = Selector::parse("td").unwrap();

    let stocks = document
        .select(&td_selector)
        .filter_map(|td| {
            let cell = td.text().collect::<String>();
            let cell = cell.trim();
            // 代號和名稱用全形空格（\u{3000}）分隔
            let (code, name) = cell.split_once('\u{3000}')?;
            let code = code.trim().to_string();
            let name = name.trim().to_string();
            // 只保留純數字代號且長度 <= 5（排除權證 700xxx 等）
            if code.chars().all(|c| c.is_ascii_digit()) && code.len() <= 5 && !name.is_empty() {
                Some(Stock { code, name, market: Market::Otc })
            } else {
                None
            }
        })
        .collect();

    Ok(stocks)
}

/// 從 yfinance symbol 取出純股票代號
pub fn code_from_symbol(symbol: &str) -> &str {
    symbol
        .strip_suffix(".TWO")
        .or_else(|| symbol.strip_suffix(".TW"))
        .unwrap_or(symbol)
}
