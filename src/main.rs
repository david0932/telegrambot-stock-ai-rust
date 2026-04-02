// main.rs - 程式入口
//
// 對應 Python 的 main.py
// 負責初始化 logger、載入設定、啟動 bot

mod alerts;
mod analyzer;
mod bot;
mod config;
mod fetcher;
mod formatter;
mod scheduler;
mod searcher;
mod user_store;

#[tokio::main]
async fn main() {
    // 初始化 logger（對應 Python 的 logging.basicConfig）
    env_logger::init();

    log::info!("啟動台股看盤 Bot...");

    // 載入設定檔
    let config = config::load_config("config.json")
        .expect("無法載入 config.json，請確認檔案存在且格式正確");

    // 啟動 bot（內含排程器）
    bot::run(config).await;
}
