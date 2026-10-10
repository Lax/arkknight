//! 游戏日界与时间窗纯函数（§10.3）。
//!
//! 游戏日界 = `game_day_boundary`（官服 UTC-4 的 04:00 本地表示）；时间窗
//! `HH:MM-HH:MM` 在**时钟日**内表达且不跨午夜（§10 M1 实现备注）。全部为纯函数，
//! 配 mock 时刻测试。

use chrono::{Datelike, Duration, Local, NaiveTime, Timelike};

use crate::model::{ScheduledExecutor, TimeWindow};

/// `HH:MM` → 当日分钟数。`24:00`（午夜端点）→ 1440：跨午夜时段以
/// 多段表达（如 22:00-24:00 + 00:00-02:00），此端点是其中前段的自然结尾。
pub fn hhmm_to_minutes(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if m > 59 || h > 24 || (h == 24 && m != 0) {
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

/// 当前时刻命中的窗口（账号的每日窗口表；时钟日内 [start, end)）。
/// 返回 (窗口引用, 执行器, 任务名)。
pub fn active_window(
    now: chrono::DateTime<Local>,
    windows: &[TimeWindow],
) -> Option<(&TimeWindow, ScheduledExecutor, Option<&str>)> {
    let now_min = now.time().num_minutes_from_midnight();
    windows.iter().find_map(|w| {
        let (s, e) = (hhmm_to_minutes(&w.start)?, hhmm_to_minutes(&w.end)?);
        if s < e && now_min >= s && now_min < e {
            Some((w, w.executor, w.task.as_deref()))
        } else {
            None
        }
    })
}

/// 距离窗口开始还有多久（未开始时 Some(duration)；已开始/已结束 None）。
pub fn until_window_start(now: chrono::DateTime<Local>, w: &TimeWindow) -> Option<Duration> {
    let (s, _) = (hhmm_to_minutes(&w.start)?, hhmm_to_minutes(&w.end)?);
    let start_today = now
        .date_naive()
        .and_hms_opt(s / 60, s % 60, 0)?
        .and_local_timezone(Local)
        .single()?;
    if now < start_today {
        Some(start_today - now)
    } else {
        None
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
        assert_eq!(hhmm_to_minutes("24:00"), Some(1440), "午夜端点合法");
        assert_eq!(hhmm_to_minutes("24:30"), None, "24 只允许整点");
        // 解析层宽松（单位数字可过）；严格 HH:MM 由模型校验层拒绝
        assert_eq!(hhmm_to_minutes("4:00"), Some(240));
    }

    #[test]
    fn window_may_end_at_midnight() {
        // 22:00-24:00 合法且 [start,end) 语义成立：23:59 命中、午夜后不命中
        let windows = [window("22:00", "24:00", ScheduledExecutor::Mower)];
        assert!(active_window(at(2026, 10, 9, 23, 59), &windows).is_some());
        assert!(active_window(at(2026, 10, 9, 21, 59), &windows).is_none());
        // 跨午夜时段 = 两段：22:00-24:00 + 00:00-02:00
        let split = [
            window("22:00", "24:00", ScheduledExecutor::Mower),
            window("00:00", "02:00", ScheduledExecutor::Mower),
        ];
        assert!(active_window(at(2026, 10, 9, 23, 30), &split).is_some());
        assert!(active_window(at(2026, 10, 10, 1, 0), &split).is_some());
        assert!(active_window(at(2026, 10, 10, 2, 1), &split).is_none());
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
    fn wrap_window_never_matches() {
        // 跨午夜窗口在配置校验即拒绝；此处兜底：s>=e 不命中
        let windows = [window("23:00", "02:00", ScheduledExecutor::Mower)];
        assert!(active_window(at(2026, 10, 9, 23, 30), &windows).is_none());
    }

    #[test]
    fn until_start() {
        let w = window("08:00", "12:00", ScheduledExecutor::Mower);
        assert_eq!(
            until_window_start(at(2026, 10, 9, 7, 0), &w),
            Some(Duration::hours(1))
        );
        assert_eq!(until_window_start(at(2026, 10, 9, 8, 0), &w), None);
    }
}
