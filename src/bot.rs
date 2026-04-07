// bot.rs - Telegram Bot 主程式與所有指令 handler
//
// 對應 Python 的 src/bot.py
// 使用 teloxide 框架，以 macro 定義指令

use crate::alerts::AlertEngine;
use crate::analyzer;
use crate::config::{Config, save_config};
use crate::fetcher::{fetch_history, fetch_quote};
use crate::formatter::{format_alert, format_quote, format_summary};
use crate::scheduler::create_scheduler;
use crate::searcher::{code_from_symbol, get_stock_name, resolve_symbol, search_stocks};
use crate::user_store::{get_user, save_user, AlertCondition};
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use teloxide::utils::command::BotCommands;
use tokio::sync::Mutex;

// ── 指令定義 ────────────────────────────────────────────────────

/// 對應 Python 各 cmd_* 函式的指令枚舉
/// teloxide 的 #[command] macro 自動解析 Telegram 訊息
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum Command {
    #[command(description = "申請使用權限")]
    Start,
    #[command(description = "顯示指令說明")]
    Help,
    #[command(description = "搜尋股票代號或名稱")]
    Search(String),
    #[command(description = "查詢即時行情")]
    Price(String),
    #[command(description = "查看追蹤清單")]
    List,
    #[command(description = "新增追蹤股票")]
    Add(String),
    #[command(description = "移除追蹤股票")]
    Remove(String),
    #[command(description = "設定價格警示")]
    Alert(String),
    #[command(description = "手動推送摘要")]
    Summary,
    #[command(description = "AI 分析")]
    Analyze(String),
    #[command(description = "開關定時推播")]
    Interval(String),
    #[command(description = "查看運作狀態")]
    Status,
    // 管理員指令
    #[command(description = "核准用戶")]
    Allow(String),
    #[command(description = "拒絕申請")]
    Deny(String),
    #[command(description = "撤銷授權")]
    Revoke(String),
    #[command(description = "列出所有用戶")]
    Users,
}

// ── 共用狀態 ─────────────────────────────────────────────────────

/// 各 handler 共用的狀態（透過 Arc<Mutex<...>> 跨 async 任務共享）
struct AppState {
    config_path: String,
    config: Config,
    alert_engines: HashMap<String, AlertEngine>,
}

type State = Arc<Mutex<AppState>>;

// ── 入口 ─────────────────────────────────────────────────────────

pub async fn run(config: Config) {
    let bot = Bot::new(&config.telegram_token);
    let config_path = "config.json".to_string();

    let state = Arc::new(Mutex::new(AppState {
        config_path: config_path.clone(),
        config: config.clone(),
        alert_engines: HashMap::new(),
    }));

    // 啟動排程器
    {
        let bot_clone = bot.clone();
        let state_clone = state.clone();

        // 間隔推播（每 N 分鐘，respect per-user interval_enabled）
        let bot_interval = bot_clone.clone();
        let state_interval = state_clone.clone();
        let interval_job = move || {
            let bot = bot_interval.clone();
            let state = state_interval.clone();
            tokio::spawn(async move {
                push_summary_broadcast(&bot, &state, true).await;
            });
        };

        // 關鍵時間點推播（無視 interval_enabled）
        let bot_keytime = bot_clone.clone();
        let state_keytime = state_clone.clone();
        let key_time_job = move || {
            let bot = bot_keytime.clone();
            let state = state_keytime.clone();
            tokio::spawn(async move {
                push_summary_broadcast(&bot, &state, false).await;
            });
        };

        let interval_minutes = config.schedule.interval_minutes;
        let key_times = config.schedule.key_times.clone();

        tokio::spawn(async move {
            match create_scheduler(interval_minutes, &key_times, interval_job, key_time_job).await {
                Ok(sched) => {
                    if let Err(e) = sched.start().await {
                        log::error!("排程器啟動失敗：{e}");
                    }
                }
                Err(e) => log::error!("無法建立排程器：{e}"),
            }
        });
    }

    // 啟動 bot polling
    let handler = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(handle_command);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

// ── 指令分派 ─────────────────────────────────────────────────────

async fn handle_command(bot: Bot, msg: Message, cmd: Command, state: State) -> ResponseResult<()> {
    let chat_id = msg.chat.id.to_string();

    // 授權檢查（Start 和 Help 不需要）
    let needs_auth = !matches!(cmd, Command::Start | Command::Help);
    if needs_auth {
        let st = state.lock().await;
        if !st.config.is_allowed(&chat_id) {
            bot.send_message(msg.chat.id, "⛔ 您尚未獲得授權，請傳送 /start 申請使用權限。")
                .await?;
            return Ok(());
        }
    }

    match cmd {
        Command::Start        => cmd_start(&bot, &msg, &chat_id, &state).await?,
        Command::Help         => cmd_help(&bot, &msg, &chat_id, &state).await?,
        Command::Search(q)    => cmd_search(&bot, &msg, &q).await?,
        Command::Price(code)  => cmd_price(&bot, &msg, &code).await?,
        Command::List         => cmd_list(&bot, &msg, &chat_id).await?,
        Command::Add(code)    => cmd_add(&bot, &msg, &chat_id, &code).await?,
        Command::Remove(code) => cmd_remove(&bot, &msg, &chat_id, &code).await?,
        Command::Alert(args)  => cmd_alert(&bot, &msg, &chat_id, &args).await?,
        Command::Summary      => cmd_summary(&bot, &msg, &chat_id).await?,
        Command::Analyze(arg) => cmd_analyze(&bot, &msg, &chat_id, &arg, &state).await?,
        Command::Interval(s)  => cmd_interval(&bot, &msg, &chat_id, &s, &state).await?,
        Command::Status       => cmd_status(&bot, &msg, &chat_id, &state).await?,
        Command::Allow(id)    => cmd_allow(&bot, &msg, &chat_id, &id, &state).await?,
        Command::Deny(id)     => cmd_deny(&bot, &msg, &chat_id, &id).await?,
        Command::Revoke(id)   => cmd_revoke(&bot, &msg, &chat_id, &id, &state).await?,
        Command::Users        => cmd_users(&bot, &msg, &chat_id, &state).await?,
    }

    Ok(())
}

// ── 各指令實作 ────────────────────────────────────────────────────
// 每個函式對應 Python 的一個 cmd_* 函式

async fn cmd_start(bot: &Bot, msg: &Message, chat_id: &str, state: &State) -> ResponseResult<()> {
    let st = state.lock().await;
    if st.config.is_allowed(chat_id) {
        bot.send_message(msg.chat.id, "✅ 您已獲得授權，輸入 /help 查看指令。").await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "📋 申請已送出，等待管理員審核。").await?;

    let user = msg.from();
    let name = user.map(|u: &teloxide::types::User| u.full_name()).unwrap_or_else(|| chat_id.to_string());
    let username = user
        .and_then(|u| u.username.as_deref())
        .map(|u| format!("@{}", u))
        .unwrap_or_else(|| "（無用戶名）".to_string());

    let notify = format!(
        "📩 新用戶申請\n姓名：{name} {username}\nchat\\_id：`{chat_id}`\n\n/allow {chat_id} — 核准\n/deny {chat_id} — 拒絕"
    );

    for admin_id in &st.config.admin_ids {
        let _ = bot
            .send_message(ChatId(admin_id.parse().unwrap_or_default()), &notify)
            .parse_mode(ParseMode::Markdown)
            .await;
    }

    Ok(())
}

async fn cmd_help(bot: &Bot, msg: &Message, chat_id: &str, state: &State) -> ResponseResult<()> {
    let st = state.lock().await;
    let mut text = "\
📖 *指令說明*\n\n\
/search 台積電 — 搜尋股票代號或名稱\n\
/price 2330 — 查詢即時行情\n\
/summary — 推送所有追蹤股票摘要\n\
/list — 查看追蹤清單與警示條件\n\
/add 2330 — 新增追蹤股票\n\
/remove 2330 — 移除追蹤股票\n\
/alert 2330 price\\_below 900.0 — 設定警示\n\
　條件：price\\_below, price\\_above\n\
　　　　change\\_pct\\_above, change\\_pct\\_below\n\
　　　　volume\\_above\n\
/analyze [股票代號] — AI 分析\n\
/interval on|off — 開關定時推播\n\
/status — 查看運作狀態\n\
/help — 顯示此說明"
        .to_string();

    if st.config.is_admin(chat_id) {
        text.push_str(
            "\n\n👑 *管理員指令*\n\
             /allow {chat\\_id} — 核准用戶申請\n\
             /deny {chat\\_id} — 拒絕用戶申請\n\
             /revoke {chat\\_id} — 撤銷授權\n\
             /users — 列出所有授權用戶",
        );
    }

    bot.send_message(msg.chat.id, text)
        .parse_mode(ParseMode::Markdown)
        .await?;
    Ok(())
}

async fn cmd_search(bot: &Bot, msg: &Message, query: &str) -> ResponseResult<()> {
    if query.trim().is_empty() {
        bot.send_message(msg.chat.id, "用法：/search 台積電 或 /search 2330").await?;
        return Ok(());
    }
    let results = search_stocks(query, 20).await;
    if results.is_empty() {
        bot.send_message(msg.chat.id, format!("找不到符合「{query}」的股票")).await?;
        return Ok(());
    }
    let lines: Vec<String> = results
        .iter()
        .map(|s| format!("• {} {}（{}）", s.code, s.name, format!("{:?}", s.market)))
        .collect();
    bot.send_message(msg.chat.id, format!("🔍 搜尋結果：\n{}", lines.join("\n")))
        .await?;
    Ok(())
}

async fn cmd_price(bot: &Bot, msg: &Message, code: &str) -> ResponseResult<()> {
    if code.trim().is_empty() {
        bot.send_message(msg.chat.id, "用法：/price 2330").await?;
        return Ok(());
    }
    let symbol = resolve_symbol(code).await;
    match fetch_quote(&symbol).await {
        Ok(Some(mut quote)) => {
            if let Some(name) = get_stock_name(code).await {
                quote.name = name;
            }
            bot.send_message(msg.chat.id, format_quote(&quote))
                .parse_mode(ParseMode::Markdown)
                .await?;
        }
        Ok(None) => {
            log::warn!("fetch_quote({symbol}): returned None");
            bot.send_message(msg.chat.id, format!("⚠️ 無法取得 {code} 的資料")).await?;
        }
        Err(e) => {
            log::error!("fetch_quote({symbol}): {e:#}");
            bot.send_message(msg.chat.id, format!("⚠️ 無法取得 {code} 的資料")).await?;
        }
    }
    Ok(())
}

async fn cmd_list(bot: &Bot, msg: &Message, chat_id: &str) -> ResponseResult<()> {
    let user = get_user(chat_id);
    if user.stocks.is_empty() {
        bot.send_message(msg.chat.id, "追蹤清單為空，用 /add 2330 新增").await?;
        return Ok(());
    }
    let mut lines = vec![];
    for symbol in &user.stocks {
        let code = code_from_symbol(symbol);
        let name = get_stock_name(code).await.unwrap_or_else(|| code.to_string());
        let conds = user.alerts.get(symbol).cloned().unwrap_or_default();
        let cond_str = if conds.is_empty() {
            "無警示".to_string()
        } else {
            conds.iter().map(|c| format!("{}={}", c.kind, c.value)).collect::<Vec<_>>().join(", ")
        };
        lines.push(format!("• {code} {name}：{cond_str}"));
    }
    bot.send_message(msg.chat.id, format!("📋 追蹤清單：\n{}", lines.join("\n")))
        .await?;
    Ok(())
}

async fn cmd_add(bot: &Bot, msg: &Message, chat_id: &str, code: &str) -> ResponseResult<()> {
    if code.trim().is_empty() {
        bot.send_message(msg.chat.id, "用法：/add 2330").await?;
        return Ok(());
    }
    let symbol = resolve_symbol(code).await;
    let mut user = get_user(chat_id);
    if !user.stocks.contains(&symbol) {
        user.stocks.push(symbol.clone());
        let _ = save_user(chat_id, &user);
        let name = get_stock_name(code).await.unwrap_or_else(|| code.to_string());
        bot.send_message(msg.chat.id, format!("✅ 已新增 {code} {name} 到追蹤清單")).await?;
    } else {
        bot.send_message(msg.chat.id, format!("{code} 已在清單中")).await?;
    }
    Ok(())
}

async fn cmd_remove(bot: &Bot, msg: &Message, chat_id: &str, code: &str) -> ResponseResult<()> {
    if code.trim().is_empty() {
        bot.send_message(msg.chat.id, "用法：/remove 2330").await?;
        return Ok(());
    }
    let symbol = resolve_symbol(code).await;
    let mut user = get_user(chat_id);
    if user.stocks.contains(&symbol) {
        user.stocks.retain(|s| s != &symbol);
        user.alerts.remove(&symbol);
        let _ = save_user(chat_id, &user);
        bot.send_message(msg.chat.id, format!("✅ 已移除 {code}")).await?;
    } else {
        bot.send_message(msg.chat.id, format!("{code} 不在清單中")).await?;
    }
    Ok(())
}

async fn cmd_alert(bot: &Bot, msg: &Message, chat_id: &str, args: &str) -> ResponseResult<()> {
    // 格式：/alert 2330 price_below 900.0
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 3 {
        bot.send_message(
            msg.chat.id,
            "用法：/alert 2330 price_below 900.0\n條件類型：price_below, price_above, change_pct_above, change_pct_below, volume_above",
        )
        .await?;
        return Ok(());
    }
    let code = parts[0].to_uppercase();
    let kind = parts[1].to_string();
    let value: f64 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => {
            bot.send_message(msg.chat.id, "⚠️ 數值格式錯誤").await?;
            return Ok(());
        }
    };
    let symbol = resolve_symbol(&code).await;
    let mut user = get_user(chat_id);
    user.alerts.entry(symbol).or_default().push(AlertCondition { kind: kind.clone(), value });
    let _ = save_user(chat_id, &user);
    bot.send_message(msg.chat.id, format!("✅ 已設定 {code} 警示：{kind} = {value}")).await?;
    Ok(())
}

async fn cmd_summary(bot: &Bot, msg: &Message, chat_id: &str) -> ResponseResult<()> {
    let user = get_user(chat_id);
    let mut quotes = vec![];
    for symbol in &user.stocks {
        if let Ok(Some(mut q)) = fetch_quote(symbol).await {
            if let Some(name) = get_stock_name(code_from_symbol(symbol)).await {
                q.name = name;
            }
            quotes.push(q);
        }
    }
    bot.send_message(msg.chat.id, format_summary(&quotes))
        .parse_mode(ParseMode::Markdown)
        .await?;
    Ok(())
}

async fn cmd_analyze(bot: &Bot, msg: &Message, chat_id: &str, arg: &str, state: &State) -> ResponseResult<()> {
    let api_key = {
        let st = state.lock().await;
        st.config.gemini_api_key.clone()
    };
    if api_key.is_empty() || api_key == "YOUR_GEMINI_API_KEY" {
        bot.send_message(msg.chat.id, "⚠️ 請先在 config.json 設定 gemini_api_key").await?;
        return Ok(());
    }
    let model = {
        let st = state.lock().await;
        st.config.gemini_model.clone()
    };

    let user = get_user(chat_id);
    let symbols: Vec<String> = if arg.trim().is_empty() {
        user.stocks.clone()
    } else {
        vec![resolve_symbol(&arg.trim().to_uppercase()).await]
    };

    for symbol in &symbols {
        let code = code_from_symbol(symbol);
        let mut quote = match fetch_quote(symbol).await {
            Ok(Some(q)) => q,
            _ => {
                bot.send_message(msg.chat.id, format!("⚠️ 無法取得 {code} 報價，分析取消")).await?;
                continue;
            }
        };
        if let Some(name) = get_stock_name(code).await {
            quote.name = name;
        }
        let display = if !quote.name.is_empty() { format!("{code} {}", quote.name) } else { code.to_string() };
        bot.send_message(msg.chat.id, format!("⏳ 正在分析 {display}，請稍候...")).await?;

        let history = fetch_history(symbol, 10).await.ok().flatten();
        match analyzer::analyze(symbol, &quote, history.as_ref(), &api_key, &model).await {
            Ok(result) => { bot.send_message(msg.chat.id, result).await?; }
            Err(e) => { bot.send_message(msg.chat.id, format!("⚠️ AI 分析失敗：{e}")).await?; }
        }
    }
    Ok(())
}

async fn cmd_interval(bot: &Bot, msg: &Message, chat_id: &str, arg: &str, _state: &State) -> ResponseResult<()> {
    let enabled = match arg.trim().to_lowercase().as_str() {
        "on"  => true,
        "off" => false,
        _ => {
            bot.send_message(msg.chat.id, "用法：/interval on 或 /interval off").await?;
            return Ok(());
        }
    };
    let mut user = get_user(chat_id);
    user.schedule.interval_enabled = enabled;
    let _ = save_user(chat_id, &user);
    let status = if enabled { "已開啟 ✅" } else { "已關閉 ❌" };
    bot.send_message(msg.chat.id, format!("定時推播{status}")).await?;
    Ok(())
}

async fn cmd_status(bot: &Bot, msg: &Message, chat_id: &str, state: &State) -> ResponseResult<()> {
    let user = get_user(chat_id);
    let st = state.lock().await;
    let count = user.stocks.len();
    let interval = st.config.schedule.interval_minutes;
    let interval_enabled = user.schedule.interval_enabled;
    let key_times = st.config.schedule.key_times.join(", ");
    let interval_str = format!("每 {interval} 分鐘 {}", if interval_enabled { "✅" } else { "❌" });
    bot.send_message(
        msg.chat.id,
        format!("🤖 Bot 運作中\n追蹤股票：{count} 檔\n定時推播：{interval_str}\n關鍵時間：{key_times}"),
    )
    .await?;
    Ok(())
}

// ── 管理員指令 ───────────────────────────────────────────────────

async fn cmd_allow(bot: &Bot, msg: &Message, chat_id: &str, target_id: &str, state: &State) -> ResponseResult<()> {
    if !state.lock().await.config.is_admin(chat_id) {
        bot.send_message(msg.chat.id, "⛔ 此指令僅限管理員使用。").await?;
        return Ok(());
    }
    let target = target_id.trim();
    {
        let mut st = state.lock().await;
        if st.config.is_allowed(target) {
            bot.send_message(msg.chat.id, format!("⚠️ {target} 已在授權清單中")).await?;
            return Ok(());
        }
        st.config.add_user(target);
        let _ = save_config(&st.config_path, &st.config);
    }
    bot.send_message(msg.chat.id, format!("✅ 已核准 {target}")).await?;
    let _ = bot.send_message(ChatId(target.parse().unwrap_or_default()), "✅ 您的申請已獲核准，歡迎使用！輸入 /help 查看指令。").await;
    Ok(())
}

async fn cmd_deny(bot: &Bot, msg: &Message, chat_id: &str, target_id: &str) -> ResponseResult<()> {
    // 注意：deny 不需要從 state 讀取，只需要送訊息
    let _ = chat_id; // require_admin 在 handle_command 已檢查
    let target = target_id.trim();
    bot.send_message(msg.chat.id, format!("✅ 已拒絕 {target} 的申請")).await?;
    let _ = bot.send_message(ChatId(target.parse().unwrap_or_default()), "❌ 您的申請未獲核准。").await;
    Ok(())
}

async fn cmd_revoke(bot: &Bot, msg: &Message, chat_id: &str, target_id: &str, state: &State) -> ResponseResult<()> {
    if !state.lock().await.config.is_admin(chat_id) {
        bot.send_message(msg.chat.id, "⛔ 此指令僅限管理員使用。").await?;
        return Ok(());
    }
    let target = target_id.trim();
    {
        let mut st = state.lock().await;
        if !st.config.is_allowed(target) {
            bot.send_message(msg.chat.id, format!("⚠️ {target} 不在授權清單中")).await?;
            return Ok(());
        }
        st.config.remove_user(target);
        let _ = save_config(&st.config_path, &st.config);
    }
    bot.send_message(msg.chat.id, format!("✅ 已撤銷 {target} 的授權")).await?;
    Ok(())
}

async fn cmd_users(bot: &Bot, msg: &Message, chat_id: &str, state: &State) -> ResponseResult<()> {
    if !state.lock().await.config.is_admin(chat_id) {
        bot.send_message(msg.chat.id, "⛔ 此指令僅限管理員使用。").await?;
        return Ok(());
    }
    let st = state.lock().await;
    let allowed = &st.config.allowed_users;
    if allowed.is_empty() {
        bot.send_message(msg.chat.id, "目前無授權用戶").await?;
        return Ok(());
    }
    let mut lines = vec![];
    for uid in allowed {
        let user_data = get_user(uid);
        let stock_count = user_data.stocks.len();
        let admin_mark = if st.config.is_admin(uid) { " 👑" } else { "" };
        lines.push(format!("• {uid}{admin_mark}：{stock_count} 檔股票"));
    }
    bot.send_message(msg.chat.id, format!("👥 授權用戶清單：\n{}", lines.join("\n"))).await?;
    Ok(())
}

// ── 定時推播 ─────────────────────────────────────────────────────

/// 判斷目前是否在台灣股市交易時間內（週一至週五 09:00–13:35 UTC+8）
fn is_taiwan_market_open() -> bool {
    use chrono::{Datelike, FixedOffset, Timelike, Utc, Weekday};
    let taipei = FixedOffset::east_opt(8 * 3600).unwrap();
    let now = Utc::now().with_timezone(&taipei);
    if matches!(now.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    let mins = now.hour() * 60 + now.minute();
    // 09:00 = 540, 13:35 = 815
    mins >= 540 && mins <= 815
}

async fn push_summary_broadcast(bot: &Bot, state: &State, check_interval: bool) {
    if !is_taiwan_market_open() {
        return;
    }
    let config = state.lock().await.config.clone();

    for chat_id in &config.allowed_users {
        let user = get_user(chat_id);

        if check_interval && !user.schedule.interval_enabled {
            continue;
        }
        if user.stocks.is_empty() {
            continue;
        }

        let mut quotes = vec![];
        for symbol in &user.stocks {
            if let Ok(Some(mut q)) = fetch_quote(symbol).await {
                if let Some(name) = get_stock_name(code_from_symbol(symbol)).await {
                    q.name = name;
                }
                quotes.push(q);
            }
        }
        if quotes.is_empty() {
            continue;
        }

        let chat = ChatId(chat_id.parse().unwrap_or_default());
        if let Err(e) = bot.send_message(chat, format_summary(&quotes))
            .parse_mode(ParseMode::Markdown)
            .await
        {
            log::error!("push_summary error for {chat_id}: {e}");
            continue;
        }

        // 警示檢查
        let mut st = state.lock().await;
        let engine = st.alert_engines.entry(chat_id.clone()).or_default();
        for quote in &quotes {
            let conds = user.alerts.get(&quote.symbol).cloned().unwrap_or_default();
            let triggered = engine.check(&quote.symbol, quote, &conds);
            for cond in triggered {
                let msg_text = format_alert(quote, &cond);
                let _ = bot.send_message(chat, msg_text)
                    .parse_mode(ParseMode::Markdown)
                    .await;
            }
        }
    }
}
