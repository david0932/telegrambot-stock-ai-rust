// alerts.rs - 警示條件判斷與冷卻機制
//
// 對應 Python 的 src/alerts.py
// 每個股票觸發後進入 30 分鐘冷卻，避免洗版

use crate::fetcher::Quote;
use crate::user_store::AlertCondition;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const COOLDOWN: Duration = Duration::from_secs(30 * 60); // 30 分鐘

/// 警示引擎（每位用戶一個實例）
///
/// # 對應 Python
/// ```python
/// class AlertEngine:
///     def __init__(self):
///         self._last_triggered = {}
///     def check(self, symbol, quote, conditions) -> list:
///         ...
/// ```
pub struct AlertEngine {
    last_triggered: HashMap<String, Instant>, // key: "symbol:condition_type"
}

impl AlertEngine {
    pub fn new() -> Self {
        Self {
            last_triggered: HashMap::new(),
        }
    }

    /// 檢查所有條件，回傳觸發的條件清單
    pub fn check(&mut self, symbol: &str, quote: &Quote, conditions: &[AlertCondition]) -> Vec<AlertCondition> {
        let mut triggered = vec![];

        for cond in conditions {
            if !self.is_triggered(quote, cond) {
                continue;
            }

            let key = format!("{}:{}", symbol, cond.kind);
            let in_cooldown = self.last_triggered
                .get(&key)
                .map(|t| t.elapsed() < COOLDOWN)
                .unwrap_or(false);

            if !in_cooldown {
                self.last_triggered.insert(key, Instant::now());
                triggered.push(cond.clone());
            }
        }

        triggered
    }

    fn is_triggered(&self, quote: &Quote, cond: &AlertCondition) -> bool {
        match cond.kind.as_str() {
            "price_below"       => quote.price < cond.value,
            "price_above"       => quote.price > cond.value,
            "change_pct_above"  => quote.change_pct > cond.value,
            "change_pct_below"  => quote.change_pct < -cond.value.abs(),
            "volume_above"      => quote.volume as f64 > cond.value,
            _ => false,
        }
    }
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new()
    }
}
