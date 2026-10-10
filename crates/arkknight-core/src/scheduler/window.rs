//! 游戏日界与时间窗纯函数（§10.3）。
//!
//! 游戏日界 = `game_day_boundary`（官服 UTC-4 的 04:00 本地表示），用于游戏日
//! 归属记账（daily_guarantee、统计）与时间轴展示。时间窗按**本地时钟**表达：
//! `end < start` 即跨过自然午夜（如 `22:00-02:00`），无需拆段。全部为纯函数，
//! 配 mock 时刻测试。

use chrono::{Datelike, Duration, Local, NaiveTime, Timelike};

use crate::model::{ScheduledExecutor, TimeWindow};

/// `HH:MM` → 当日分钟数（00:00-23:59；午夜端点写作 00:00，配合 end < start 跨午夜）。
pub fn hhmm_to_minutes(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

/// 今天的游戏日界时刻（本地）：若当前时刻早于日界，则日界在昨天。
pub fn game_day_start(
    now: chrono::DateTime<Local>,
    boundary: &str,
) -> Option<chrono::DateTime<Local>> {
    let b = hhmm_to_minutes(boundary)?;
    let today_boundary = now
        .date_naive()
        .and_hms_opt(b / 60, b % 60, 0)?
        .and_local_timezone(Local)
        .single()?;
    if now >= today_boundary {
        Some(today_boundary)
    } else {
        Some(today_boundary - Duration::days(1))
    }
}

/// 当前时刻命中的窗口（账号的每日窗口表；本地时钟 [start, end)，
/// `end < start` 表示跨过午夜：命中 `now >= s 或 now < e`）。
/// 返回 (窗口引用, 执行器, 任务名)。
pub fn active_window(
    now: chrono::DateTime<Local>,
    windows: &[TimeWindow],
) -> Option<(&TimeWindow, ScheduledExecutor, Option<&str>)> {
    let now_min = now.time().num_minutes_from_midnight();
    windows.iter().find_map(|w| {
        let (s, e) = (hhmm_to_minutes(&w.start)?, hhmm_to_minutes(&w.end)?);
        let hit = if s < e {
            now_min >= s && now_min < e
        } else if s > e {
            now_min >= s || now_min < e
        } else {
            false // start == end：校验拒绝；此处兜底不命中
        };
        hit.then_some((w, w.executor, w.task.as_deref()))
    })
}

/// 距离窗口下一次开始还有多久。今天已开过（含正在开）→ 明天同一 clock 时刻。
pub fn until_window_start(now: chrono::DateTime<Local>, w: &TimeWindow) -> Option<Duration> {
    let s = hhmm_to_minutes(&w.start)?;
    let start_today = now
        .date_naive()
        .and_hms_opt(s / 60, s % 60, 0)?
        .and_local_timezone(Local)
        .single()?;
    let in_window = active_window(now, std::slice::from_ref(w)).is_some();
    if in_window {
        // 正在进行的这次开始于「今天或昨天的 clock s」（跨自然天窗口的尾段在凌晨），
        // 下一次开始 = 那个时刻 + 1 天
        let ongoing = if start_today <= now {
            start_today
        } else {
            start_today - Duration::days(1)
        };
        Some(ongoing + Duration::days(1) - now)
    } else if now < start_today {
        Some(start_today - now)
    } else {
        Some(start_today + Duration::days(1) - now)
    }
}

/// 当前游戏日的字符串键（`YYYY-MM-DD`，以日界所在日计）。
pub fn game_day_key(now: chrono::DateTime<Local>, boundary: &str) -> Option<String> {
    let start = game_day_start(now, boundary)?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        start.year(),
        start.month(),
        start.day()
    ))
}

trait NumMinutes {
    fn num_minutes_from_midnight(&self) -> u32;
}

impl NumMinutes for NaiveTime {
    fn num_minutes_from_midnight(&self) -> u32 {
        self.hour() * 60 + self.minute()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn window(start: &str, end: &str, executor: ScheduledExecutor) -> TimeWindow {
        TimeWindow {
            start: start.into(),
            end: end.into(),
            executor,
            task: None,
        }
    }

    #[test]
    fn hhmm_parse() {
        assert_eq!(hhmm_to_minutes("04:00"), Some(240));
        assert_eq!(hhmm_to_minutes("23:59"), Some(1439));
        assert_eq!(hhmm_to_minutes("24:00"), None, "午夜端点写作 00:00");
        // 解析层宽松（单位数字可过）；严格 HH:MM 由模型校验层拒绝
        assert_eq!(hhmm_to_minutes("4:00"), Some(240));
    }

    #[test]
    fn game_day_boundary_crosses() {
        let boundary = "04:00";
        // 08:00 → 今天 04:00 起
        assert_eq!(
            game_day_start(at(2026, 10, 9, 8, 0), boundary),
            Some(at(2026, 10, 9, 4, 0))
        );
        // 02:00（早于日界）→ 昨天 04:00 起
        assert_eq!(
            game_day_start(at(2026, 10, 9, 2, 0), boundary),
            Some(at(2026, 10, 8, 4, 0))
        );
        // 恰在日界
        assert_eq!(
            game_day_start(at(2026, 10, 9, 4, 0), boundary),
            Some(at(2026, 10, 9, 4, 0))
        );
        assert_eq!(
            game_day_key(at(2026, 10, 9, 2, 0), boundary).as_deref(),
            Some("2026-10-08")
        );
    }

    #[test]
    fn window_membership_half_open() {
        let windows = [window("08:00", "12:00", ScheduledExecutor::Mower)];
        // [start, end)：起点在内，终点在外
        assert!(active_window(at(2026, 10, 9, 8, 0), &windows).is_some());
        assert!(active_window(at(2026, 10, 9, 11, 59), &windows).is_some());
        assert!(active_window(at(2026, 10, 9, 12, 0), &windows).is_none());
        assert!(active_window(at(2026, 10, 9, 7, 59), &windows).is_none());
    }

    #[test]
    fn cross_midnight_window_matches() {
        // 跨自然天窗口 22:00-02:00：end < start 即跨过午夜，凌晨尾段照常命中
        let windows = [window("22:00", "02:00", ScheduledExecutor::Mower)];
        assert!(active_window(at(2026, 10, 9, 22, 0), &windows).is_some());
        assert!(active_window(at(2026, 10, 9, 23, 30), &windows).is_some());
        assert!(active_window(at(2026, 10, 10, 1, 59), &windows).is_some());
        assert!(active_window(at(2026, 10, 10, 2, 0), &windows).is_none());
        assert!(active_window(at(2026, 10, 9, 21, 59), &windows).is_none());
        // 全天窗口 00:00-23:59 依旧全天（00:00 起点含午夜）
        let allday = [window("00:00", "23:59", ScheduledExecutor::Mower)];
        assert!(active_window(at(2026, 10, 9, 3, 0), &allday).is_some());
        assert!(active_window(at(2026, 10, 9, 12, 0), &allday).is_some());
    }

    #[test]
    fn until_start() {
        let w = window("08:00", "12:00", ScheduledExecutor::Mower);
        assert_eq!(
            until_window_start(at(2026, 10, 9, 7, 0), &w),
            Some(Duration::hours(1))
        );
        // 窗口进行中 → 下一次开始是明天
        assert_eq!(
            until_window_start(at(2026, 10, 9, 9, 0), &w),
            Some(Duration::hours(23))
        );
        // 今天已结束 → 明天
        assert_eq!(
            until_window_start(at(2026, 10, 9, 13, 0), &w),
            Some(Duration::hours(19))
        );
    }

    #[test]
    fn until_start_cross_midnight() {
        let w = window("22:00", "02:00", ScheduledExecutor::Mower);
        // 凌晨 01:00 正在窗口内（本次开始于昨晚）→ 明晚 22:00
        assert_eq!(
            until_window_start(at(2026, 10, 10, 1, 0), &w),
            Some(Duration::hours(21))
        );
        // 下午 → 今晚 22:00
        assert_eq!(
            until_window_start(at(2026, 10, 9, 15, 0), &w),
            Some(Duration::hours(7))
        );
    }
}
