// formatter.rs - Telegram 訊息格式化
//
// 對應 Python 的 src/formatter.py
// 將報價資料轉為 Telegram Markdown 格式文字

use crate::fetcher::Quote;
use crate::searcher::code_from_symbol;
use crate::user_store::AlertCondition;

/// 格式化單一股票報價
///
/// # 對應 Python
/// ```python
/// def format_quote(quote: dict) -> str:
///     title = f"{code} {name}"
///     return f"📊 *{title}*\n現價：`{price:.2f}` ..."
/// ```
pub fn format_quote(quote: &Quote) -> String {
    let code = code_from_symbol(&quote.symbol);
    let name = &quote.name;
    let title = if !name.is_empty() && name != code {
        format!("{} {}", code, name)
    } else {
        code.to_string()
    };

    let arrow = if quote.change_pct >= 0.0 { "▲" } else { "▼" };
    let sign = if quote.change >= 0.0 { "+" } else { "" };

    format!(
        "📊 *{title}*\n\
         現價：`{price:.2}` {arrow} {sign}{change:.2} ({sign}{pct:.2}%)\n\
         開/高/低：{open:.2} / {high:.2} / {low:.2}\n\
         成交量：{volume} 張",
        price = quote.price,
        change = quote.change,
        pct = quote.change_pct,
        open = quote.open,
        high = quote.high,
        low = quote.low,
        volume = format_number(quote.volume),
    )
}

/// 格式化多檔股票摘要
pub fn format_summary(quotes: &[Quote]) -> String {
    if quotes.is_empty() {
        return "⚠️ 目前無法取得行情資料".to_string();
    }
    quotes
        .iter()
        .map(format_quote)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 格式化警示訊息
///
/// # 對應 Python
/// ```python
/// def format_alert(quote, alert_type, threshold) -> str:
///     ...
/// ```
pub fn format_alert(quote: &Quote, cond: &AlertCondition) -> String {
    let code = code_from_symbol(&quote.symbol);
    let desc = match cond.kind.as_str() {
        "price_below"       => format!("跌破 {:.2} 元", cond.value),
        "price_above"       => format!("突破 {:.2} 元", cond.value),
        "change_pct_above"  => format!("漲幅超過 {:.1}%", cond.value),
        "change_pct_below"  => format!("跌幅超過 {:.1}%", cond.value.abs()),
        "volume_above"      => format!("成交量超過 {} 張", format_number(cond.value as i64)),
        other               => other.to_string(),
    };

    format!("🚨 *{code} 警示*：{desc}\n\n{}", format_quote(quote))
}

fn format_number(n: i64) -> String {
    // 加千分位逗號，e.g. 8189 → "8,189"
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}
