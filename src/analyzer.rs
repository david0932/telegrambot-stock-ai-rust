// analyzer.rs - Gemini AI 技術分析
//
// 對應 Python 的 src/analyzer.py
// 沒有官方 Rust SDK，直接呼叫 Gemini REST API

use crate::fetcher::{DayBar, Quote};
use crate::searcher::code_from_symbol;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// Gemini API endpoint
// 文件：https://ai.google.dev/api/generate-content
const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

// ── Gemini API 請求/回應 JSON 結構 ──────────────────────────────

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: String,
}

// ── 公開 API ───────────────────────────────────────────────────

/// 呼叫 Gemini 進行技術分析
///
/// # 對應 Python
/// ```python
/// def analyze(symbol, quote, history, api_key, model) -> str:
///     client = genai.Client(api_key=api_key)
///     prompt = build_prompt(symbol, quote, history)
///     response = client.models.generate_content(...)
///     return response.text
/// ```
pub async fn analyze(
    symbol: &str,
    quote: &Quote,
    history: Option<&Vec<DayBar>>,
    api_key: &str,
    model: &str,
) -> Result<String> {
    let prompt = build_prompt(symbol, quote, history);
    let url = format!("{}/{}:generateContent?key={}", GEMINI_BASE, model, api_key);

    let request = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart { text: prompt }],
        }],
    };

    let client = reqwest::Client::new();
    let resp: GeminiResponse = client
        .post(&url)
        .json(&request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .unwrap_or_else(|| "無法取得分析結果".to_string());

    Ok(text)
}

/// 組裝分析 Prompt
///
/// # 對應 Python
/// ```python
/// def build_prompt(symbol, quote, history) -> str:
///     ...
/// ```
fn build_prompt(symbol: &str, quote: &Quote, history: Option<&Vec<DayBar>>) -> String {
    let code = code_from_symbol(symbol);
    let name = &quote.name;
    let stock_label = if !name.is_empty() && name != code {
        format!("{} {}", code, name)
    } else {
        code.to_string()
    };

    let instant = format!(
        "現價：{:.2} 元，漲跌：{:+.2}（{:+.2}%）\n\
         開盤：{:.2} / 最高：{:.2} / 最低：{:.2}\n\
         成交量：{} 張",
        quote.price,
        quote.change,
        quote.change_pct,
        quote.open,
        quote.high,
        quote.low,
        quote.volume,
    );

    let history_section = match history {
        Some(bars) if !bars.is_empty() => {
            let header = "日期       | 開盤  | 最高  | 最低  | 收盤  | 量(張)";
            let rows: Vec<String> = bars
                .iter()
                .map(|d| {
                    format!(
                        "{} | {:.2} | {:.2} | {:.2} | {:.2} | {}",
                        d.date, d.open, d.high, d.low, d.close, d.volume
                    )
                })
                .collect();
            format!("{}\n{}", header, rows.join("\n"))
        }
        _ => "日K資料暫時無法取得".to_string(),
    };

    format!(
        "你是一位台股技術分析師。請根據以下資料對 {stock_label} 進行分析，\
         以繁體中文撰寫，風格簡潔專業。\
         請使用「{stock_label}」稱呼該股票。\
         結尾請附上免責聲明。\n\n\
         【即時行情】\n{instant}\n\n\
         【近期走勢】\n{history_section}\n\n\
         請自由發揮你的分析。"
    )
}
