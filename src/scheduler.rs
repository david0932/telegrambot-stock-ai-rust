// scheduler.rs - 定時排程
//
// 對應 Python 的 src/scheduler.py
// 使用 tokio-cron-scheduler 實作兩種排程：
//   1. 固定間隔（每 N 分鐘）
//   2. 指定時間點（09:30, 10:30, ...）

use anyhow::Result;
use tokio_cron_scheduler::{Job, JobScheduler};

/// 建立排程器
///
/// # 參數
/// - `interval_minutes`: 定時間隔（分鐘）
/// - `key_times`: 指定推播時間，格式 "HH:MM"（台灣時間）
/// - `interval_job`: 間隔排程執行的 async fn
/// - `key_time_job`: 時間點排程執行的 async fn
///
/// # 對應 Python
/// ```python
/// def create_scheduler(interval_job, key_time_job, interval_minutes, key_times):
///     scheduler = AsyncIOScheduler(timezone="Asia/Taipei")
///     scheduler.add_job(interval_job, "interval", minutes=interval_minutes)
///     for t in key_times:
///         h, m = t.split(":")
///         scheduler.add_job(key_time_job, "cron", hour=h, minute=m)
///     return scheduler
/// ```
pub async fn create_scheduler<F, G>(
    interval_minutes: u32,
    key_times: &[String],
    interval_job: F,
    key_time_job: G,
) -> Result<JobScheduler>
where
    F: Fn() + Send + Sync + 'static + Clone,
    G: Fn() + Send + Sync + 'static + Clone,
{
    let scheduler = JobScheduler::new().await?;

    // 固定間隔排程
    // tokio-cron-scheduler 使用 cron 語法：秒 分 時 日 月 週
    // 每 N 分鐘 = "0 */{N} * * * *"
    let interval_cron = format!("0 0/{} * * * *", interval_minutes);
    {
        let job_fn = interval_job.clone();
        scheduler
            .add(Job::new_async(interval_cron.as_str(), move |_, _| {
                let f = job_fn.clone();
                Box::pin(async move { f() })
            })?)
            .await?;
    }

    // 指定時間點排程（台灣時間，TZ 由 docker-compose 的 TZ=Asia/Taipei 控制）
    for time_str in key_times {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() != 2 {
            continue;
        }
        let hour = parts[0];
        let minute = parts[1];
        // cron 格式：秒 分 時 日 月 週
        let cron = format!("0 {} {} * * *", minute, hour);

        let job_fn = key_time_job.clone();
        scheduler
            .add(Job::new_async(cron.as_str(), move |_, _| {
                let f = job_fn.clone();
                Box::pin(async move { f() })
            })?)
            .await?;
    }

    Ok(scheduler)
}
