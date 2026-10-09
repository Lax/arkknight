//! SQLite 运行态持久化（§14）：rusqlite + WAL + bundled，`state/arkreunion.db`。
//!
//! INV-4：SQLite 仅存运行态与统计；账号/设备/策略以文件树为准。
//! 时间戳列统一为 INTEGER（UTC epoch 毫秒）。
//! 设备租约持有者统一为 `holder` 字符串（`session:<id>` / `switch:<account>` /
//! `provision:<account>`）——手动 switch/provision 同样短暂持有租约（§6.5），
//! 不必伪造会话行。daemon 单写者 + standalone flock（ADR-0001 D7）由
//! [`crate::lock`] 与上层保证。

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

/// 当前 schema 版本。
///
/// - v1：初始表结构（`account_id` 列）
/// - v2：`account_id` → `account_key`（Account.id → key 改名）
pub const SCHEMA_VERSION: i64 = 2;

/// store 层错误。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("数据库错误：{0}")]
    Db(#[from] rusqlite::Error),

    #[error("设备 {device} 租约被 {holder} 持有")]
    LeaseConflict { device: String, holder: String },

    #[error("端口段 {a}-{b} 已无空闲端口")]
    PortExhausted { a: u16, b: u16 },

    #[error("会话 {0} 不存在")]
    SessionNotFound(u64),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// UTC epoch 毫秒。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 会话行（`sessions` 表）。
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: u64,
    pub account_key: String,
    pub device_name: String,
    pub executor: String,
    pub runner: Option<String>,
    pub state: String,
    pub mower_port: Option<u16>,
    /// 执行器句柄串（`pid=<n> port=<p>` 等；跨进程 stop 依赖）
    pub locator: Option<String>,
    pub slice_deadline_ms: Option<i64>,
    pub max_runtime_deadline_ms: Option<i64>,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    pub outcome: Option<String>,
    pub error: Option<String>,
}

/// 会话事件条目。
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub ts_ms: i64,
    pub kind: String,
    pub detail: Option<String>,
}

/// 切号日志条目。
#[derive(Debug, Clone)]
pub struct SwitchLogEntry {
    pub ts_ms: i64,
    pub account_key: String,
    pub device_name: String,
    pub ok: bool,
    pub duration_ms: i64,
    pub retries: u32,
    pub maa_log_excerpt: Option<String>,
}

/// Store：单连接 + 互斥（daemon 单写者；读方也可共用此连接）。
#[derive(Debug)]
pub struct Store {
    conn: std::sync::Mutex<Connection>,
}

/// 建会话入参（[`Store::create_session`]）。
#[derive(Debug, Clone)]
pub struct NewSession<'a> {
    pub account_key: &'a str,
    pub device_name: &'a str,
    pub executor: &'a str,
    pub runner: Option<&'a str>,
    pub state: &'a str,
    pub mower_port: Option<u16>,
    pub slice_deadline_ms: Option<i64>,
    pub max_runtime_deadline_ms: Option<i64>,
}

/// schema v1 建表语句（设计文档 §14）。
///
/// 注意：此处保持 `account_id` 原样——v1→v2 迁移负责改名，新库直接建 v1 再迁移，
/// 保证「建表」与「迁移」两条路径产生同一 schema。
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta(
  key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE IF NOT EXISTS sessions(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL, device_name TEXT NOT NULL,
  executor TEXT NOT NULL, runner TEXT, state TEXT NOT NULL,
  mower_port INTEGER, locator TEXT,
  slice_deadline_ms INTEGER, max_runtime_deadline_ms INTEGER,
  started_at_ms INTEGER, ended_at_ms INTEGER, outcome TEXT, error TEXT);
CREATE INDEX IF NOT EXISTS idx_sessions_device_state ON sessions(device_name, state);
CREATE INDEX IF NOT EXISTS idx_sessions_account_state ON sessions(account_id, state);

CREATE TABLE IF NOT EXISTS session_events(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER NOT NULL REFERENCES sessions(id),
  ts_ms INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT);
CREATE INDEX IF NOT EXISTS idx_events_session ON session_events(session_id);

CREATE TABLE IF NOT EXISTS device_leases(
  device_name TEXT PRIMARY KEY,
  holder TEXT NOT NULL,
  acquired_at_ms INTEGER NOT NULL,
  heartbeat_at_ms INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS port_allocations(
  port INTEGER PRIMARY KEY,
  holder TEXT NOT NULL, kind TEXT NOT NULL, allocated_at_ms INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS switch_log(
  id INTEGER PRIMARY KEY AUTOINCREMENT, ts_ms INTEGER NOT NULL,
  account_id TEXT NOT NULL, device_name TEXT NOT NULL,
  ok INTEGER NOT NULL, duration_ms INTEGER NOT NULL, retries INTEGER NOT NULL,
  maa_log_excerpt TEXT);

CREATE TABLE IF NOT EXISTS logins(
  account_id TEXT NOT NULL, device_name TEXT NOT NULL, status TEXT NOT NULL,
  first_at_ms INTEGER NOT NULL, last_verified_at_ms INTEGER NOT NULL,
  PRIMARY KEY(account_id, device_name));

CREATE TABLE IF NOT EXISTS maintenance_log(
  id INTEGER PRIMARY KEY AUTOINCREMENT, ts_ms INTEGER NOT NULL,
  target TEXT NOT NULL, action TEXT NOT NULL, from_version TEXT, to_version TEXT,
  ok INTEGER NOT NULL, detail TEXT);

CREATE TABLE IF NOT EXISTS stats_daily(
  day TEXT NOT NULL, account_id TEXT NOT NULL,
  minutes_run INTEGER NOT NULL DEFAULT 0, sessions_cnt INTEGER NOT NULL DEFAULT 0,
  switch_cnt INTEGER NOT NULL DEFAULT 0, sanity_spent INTEGER,
  PRIMARY KEY(day, account_id));
"#;

impl Store {
    /// 打开（必要时创建）数据库：WAL + busy_timeout + 迁移到最新 schema。
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                StoreError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let store = Store {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    /// 内存库（测试用）。
    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        let store = Store {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    /// 连接守卫（Store: Sync 的关键——rusqlite::Connection 仅 Send）。
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("store 连接锁 poisoned")
    }

    fn migrate(&self) -> Result<()> {
        let cur: i64 = self
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if cur > SCHEMA_VERSION {
            return Err(StoreError::Db(rusqlite::Error::InvalidParameterName(
                format!(
                    "数据库 schema v{cur} 高于本程序支持的 v{SCHEMA_VERSION}：请升级 arkreunion"
                ),
            )));
        }
        if cur < SCHEMA_VERSION {
            self.conn().execute_batch("BEGIN")?;
            if cur < 1 {
                self.conn().execute_batch(SCHEMA_V1)?;
                self.conn().execute(
                    "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
                    [SCHEMA_VERSION.to_string()],
                )?;
            }
            if cur < 2 {
                self.migrate_v1_to_v2()?;
            }
            self.conn()
                .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}; COMMIT"))?;
            tracing::info!(from = cur, to = SCHEMA_VERSION, "数据库 schema 迁移完成");
        }
        Ok(())
    }

    /// v1 → v2：`account_id` 列更名为 `account_key`（Account.id → key 改名）。
    ///
    /// SQLite 3.25+ 支持 `ALTER TABLE … RENAME COLUMN`；主键与索引随列自动跟随。
    fn migrate_v1_to_v2(&self) -> Result<()> {
        // 旧索引名随列更名保留但语义已对（sessions 的索引恰以 account_id 为首列），
        // 仅需改名以保持与 SCHEMA_V2 一致。
        for (table, old_index, new_index) in [
            (
                "sessions",
                "idx_sessions_account_state",
                "idx_sessions_account_key_state",
            ),
            ("switch_log", "", ""),
            ("logins", "", ""),
            ("stats_daily", "", ""),
        ] {
            self.conn().execute_batch(&format!(
                "ALTER TABLE {table} RENAME COLUMN account_id TO account_key"
            ))?;
            if !new_index.is_empty() {
                self.conn()
                    .execute_batch(&format!("DROP INDEX IF EXISTS {old_index}"))?;
                self.conn().execute_batch(&format!(
                    "CREATE INDEX IF NOT EXISTS {new_index} ON sessions(account_key, state)"
                ))?;
            }
        }
        tracing::info!(
            "已迁移：sessions/switch_log/logins/stats_daily 的 account_id 列 → account_key"
        );
        Ok(())
    }

    // ---------- 会话 ----------

    /// 创建会话，返回 id（started_at 记为当前时刻）。
    pub fn create_session(&self, s: NewSession<'_>) -> Result<u64> {
        self.conn().execute(
            "INSERT INTO sessions(account_key, device_name, executor, runner, state, mower_port,
                                  slice_deadline_ms, max_runtime_deadline_ms, started_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                s.account_key,
                s.device_name,
                s.executor,
                s.runner,
                s.state,
                s.mower_port,
                s.slice_deadline_ms,
                s.max_runtime_deadline_ms,
                now_ms()
            ],
        )?;
        Ok(self.conn().last_insert_rowid() as u64)
    }

    /// 回写端口分配结果（分配发生在建行之后：holder 需要会话 id）。
    pub fn set_mower_port(&self, id: u64, port: u16) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET mower_port = ?1 WHERE id = ?2",
            params![port as i64, id as i64],
        )?;
        Ok(())
    }

    fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
        Ok(SessionRow {
            id: r.get::<_, i64>(0)? as u64,
            account_key: r.get(1)?,
            device_name: r.get(2)?,
            executor: r.get(3)?,
            runner: r.get(4)?,
            state: r.get(5)?,
            mower_port: r.get::<_, Option<i64>>(6)?.map(|v| v as u16),
            locator: r.get(7)?,
            slice_deadline_ms: r.get(8)?,
            max_runtime_deadline_ms: r.get(9)?,
            started_at_ms: r.get(10)?,
            ended_at_ms: r.get(11)?,
            outcome: r.get(12)?,
            error: r.get(13)?,
        })
    }

    const SESSION_COLS: &str = "id, account_key, device_name, executor, runner, state, mower_port,\
                         locator, slice_deadline_ms, max_runtime_deadline_ms, started_at_ms,\
                         ended_at_ms, outcome, error";

    pub fn get_session(&self, id: u64) -> Result<Option<SessionRow>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {} FROM sessions WHERE id = ?1", Self::SESSION_COLS),
                [id as i64],
                Self::row_to_session,
            )
            .optional()?)
    }

    /// 设备上的活跃会话（非终态；设备租约前提，§6.5）。
    pub fn active_session_by_device(&self, device_name: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM sessions WHERE device_name = ?1 AND state IN
             ('created','queued','switching','running','draining')",
            Self::SESSION_COLS
        ))?;
        Ok(stmt
            .query_row([device_name], Self::row_to_session)
            .optional()?)
    }

    /// 账号的活跃会话（任意设备；一账号至多一个活跃会话，§6.5）。
    pub fn active_session_by_account_key(&self, account_key: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM sessions WHERE account_key = ?1 AND state IN
             ('created','queued','switching','running','draining')",
            Self::SESSION_COLS
        ))?;
        Ok(stmt
            .query_row([account_key], Self::row_to_session)
            .optional()?)
    }

    /// 迁移会话状态并写事件（任何状态迁移必须走这里，§10.2）。
    pub fn update_session_state(
        &self,
        id: u64,
        new_state: &str,
        event_kind: &str,
        event_detail: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE sessions SET state = ?1 WHERE id = ?2",
            params![new_state, id as i64],
        )?;
        if n == 0 {
            return Err(StoreError::SessionNotFound(id));
        }
        tx.execute(
            "INSERT INTO session_events(session_id, ts_ms, kind, detail) VALUES (?1, ?2, ?3, ?4)",
            params![id as i64, now_ms(), event_kind, event_detail],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 结束会话（终态 + 结果归因）。
    pub fn finish_session(
        &self,
        id: u64,
        state: &str,
        outcome: &str,
        error: Option<&str>,
        event_kind: &str,
    ) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let n = tx.execute(
            "UPDATE sessions SET state = ?1, outcome = ?2, error = ?3, ended_at_ms = ?4
             WHERE id = ?5",
            params![state, outcome, error, now_ms(), id as i64],
        )?;
        if n == 0 {
            return Err(StoreError::SessionNotFound(id));
        }
        tx.execute(
            "INSERT INTO session_events(session_id, ts_ms, kind, detail) VALUES (?1, ?2, ?3, ?4)",
            params![id as i64, now_ms(), event_kind, error],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 记录执行器句柄与硬上限截止时间（start 成功后调用）。
    pub fn set_executor_handle(
        &self,
        id: u64,
        locator: &str,
        slice_deadline_ms: Option<i64>,
        max_runtime_deadline_ms: Option<i64>,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET locator = ?1, slice_deadline_ms = ?2, max_runtime_deadline_ms = ?3,
             started_at_ms = COALESCE(started_at_ms, ?4) WHERE id = ?5",
            params![locator, slice_deadline_ms, max_runtime_deadline_ms, now_ms(), id as i64],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self, limit: u32) -> Result<Vec<SessionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM sessions ORDER BY id DESC LIMIT ?1",
            Self::SESSION_COLS
        ))?;
        let rows = stmt.query_map([limit], Self::row_to_session)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 账号自 `since_ms` 以来的会话数（daily_guarantee 告警用）。
    pub fn sessions_cnt_since(&self, account_key: &str, since_ms: i64) -> Result<u32> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE account_key = ?1 AND started_at_ms >= ?2",
            params![account_key, since_ms],
            |r| r.get::<_, i64>(0).map(|v| v as u32),
        )?)
    }

    pub fn session_events(&self, session_id: u64) -> Result<Vec<SessionEvent>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ts_ms, kind, detail FROM session_events WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([session_id as i64], |r| {
            Ok(SessionEvent {
                ts_ms: r.get(0)?,
                kind: r.get(1)?,
                detail: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---------- 设备租约（§6.5/§10.2） ----------

    /// 获取设备租约；已被持有则返回 [`StoreError::LeaseConflict`]。
    pub fn acquire_lease(&self, device_name: &str, holder: &str) -> Result<()> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT holder FROM device_leases WHERE device_name = ?1",
                [device_name],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            Some(h) => Err(StoreError::LeaseConflict {
                device: device_name.into(),
                holder: h,
            }),
            None => {
                tx.execute(
                    "INSERT INTO device_leases(device_name, holder, acquired_at_ms, heartbeat_at_ms)
                     VALUES (?1, ?2, ?3, ?3)",
                    params![device_name, holder, now_ms()],
                )?;
                tx.commit()?;
                Ok(())
            }
        }
    }

    pub fn release_lease(&self, device_name: &str, holder: &str) -> Result<bool> {
        let n = self.conn().execute(
            "DELETE FROM device_leases WHERE device_name = ?1 AND holder = ?2",
            params![device_name, holder],
        )?;
        Ok(n > 0)
    }

    pub fn heartbeat_lease(&self, device_name: &str, holder: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE device_leases SET heartbeat_at_ms = ?3
             WHERE device_name = ?1 AND holder = ?2",
            params![device_name, holder, now_ms()],
        )?;
        Ok(())
    }

    /// 清空全部租约并返回被清的设备名（仅限 daemon 启动时的单写者语义，
    /// 见 `scheduler::recover_on_start`；CLI 场景请用心跳超时的
    /// [`Self::recover_orphan_leases`]）。
    pub fn recover_all_leases(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT device_name FROM device_leases")?;
        let all: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if !all.is_empty() {
            conn.execute("DELETE FROM device_leases", [])?;
        }
        Ok(all)
    }

    pub fn lease_holder(&self, device_name: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT holder FROM device_leases WHERE device_name = ?1",
                [device_name],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// 回收孤儿租约（daemon 启动/崩溃恢复）：心跳超过 `timeout` 的租约释放。
    pub fn recover_orphan_leases(&self, timeout: Duration) -> Result<Vec<String>> {
        let cutoff = now_ms() - timeout.as_millis() as i64;
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT device_name FROM device_leases WHERE heartbeat_at_ms < ?1")?;
        let orphans: Vec<String> = stmt
            .query_map([cutoff], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if !orphans.is_empty() {
            conn.execute(
                "DELETE FROM device_leases WHERE heartbeat_at_ms < ?1",
                [cutoff],
            )?;
        }
        Ok(orphans)
    }

    // ---------- 端口分配（§8.3，R8） ----------

    /// 分配端口：跳过已记账端口，逐个做绑定探测；成功后记账。
    pub fn alloc_port(&self, kind: &str, holder: &str, range: [u16; 2]) -> Result<u16> {
        let [a, b] = range;
        let mut used: Vec<u16> = {
            let conn = self.conn();
            let mut stmt =
                conn.prepare("SELECT port FROM port_allocations WHERE port BETWEEN ?1 AND ?2")?;
            let rows = stmt.query_map(params![a as i64, b as i64], |r| {
                r.get::<_, i64>(0).map(|v| v as u16)
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        used.sort_unstable();
        let mut cursor = used.iter();
        let mut next_used = cursor.next();
        for port in a..=b {
            if next_used == Some(&port) {
                next_used = cursor.next();
                continue;
            }
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                let n = self.conn().execute(
                    "INSERT INTO port_allocations(port, holder, kind, allocated_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![port as i64, holder, kind, now_ms()],
                );
                match n {
                    Ok(_) => return Ok(port),
                    // 并发抢占（唯一键冲突）：尝试下一个
                    Err(rusqlite::Error::SqliteFailure(e, _))
                        if e.code == rusqlite::ErrorCode::ConstraintViolation => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Err(StoreError::PortExhausted { a, b })
    }

    pub fn release_ports(&self, holder: &str) -> Result<usize> {
        Ok(self
            .conn()
            .execute("DELETE FROM port_allocations WHERE holder = ?1", [holder])?)
    }

    // ---------- logins（亲和矩阵，§7.3/§9.2） ----------

    /// 记录「账号已在设备登录过」；保留 first_at。
    pub fn record_login(&self, account_key: &str, device_name: &str, status: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO logins(account_key, device_name, status, first_at_ms, last_verified_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(account_key, device_name)
             DO UPDATE SET status = ?3, last_verified_at_ms = ?4",
            params![account_key, device_name, status, now_ms()],
        )?;
        Ok(())
    }

    pub fn login_status(&self, account_key: &str, device_name: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT status FROM logins WHERE account_key = ?1 AND device_name = ?2",
                params![account_key, device_name],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn list_logins(&self, account_key: &str) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT device_name, status FROM logins WHERE account_key = ?1 ORDER BY device_name",
        )?;
        let rows = stmt.query_map([account_key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---------- switch_log（§9.1） ----------

    pub fn insert_switch_log(&self, e: &SwitchLogEntry) -> Result<()> {
        self.conn().execute(
            "INSERT INTO switch_log(ts_ms, account_key, device_name, ok, duration_ms, retries, maa_log_excerpt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                e.ts_ms,
                e.account_key,
                e.device_name,
                e.ok as i64,
                e.duration_ms,
                e.retries as i64,
                e.maa_log_excerpt
            ],
        )?;
        Ok(())
    }

    // ---------- maintenance_log（§12） ----------

    pub fn insert_maintenance(
        &self,
        target: &str,
        action: &str,
        from_version: Option<&str>,
        to_version: Option<&str>,
        ok: bool,
        detail: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO maintenance_log(ts_ms, target, action, from_version, to_version, ok, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![now_ms(), target, action, from_version, to_version, ok as i64, detail],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_wal_and_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("state/arkreunion.db");
        let store = Store::open(&db).unwrap();
        // 重复打开幂等
        drop(store);
        Store::open(&db).unwrap();
        let conn = Connection::open(&db).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // 高版本拒绝
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        drop(conn);
        let err = Store::open(&db).unwrap_err();
        assert!(err.to_string().contains("升级"), "{err}");
    }

    #[test]
    fn v1数据库迁移后列名与数据保留() {
        // 造一个 v1 老库（含 account_id 列与数据），验证迁移到 v2 后可读
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("state/arkreunion.db");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.execute_batch(
                "INSERT INTO sessions(account_id, device_name, executor, state, started_at_ms)
                 VALUES ('main', 'd1', 'mower', 'finished', 1000)",
            )
            .unwrap();
            conn.execute_batch("PRAGMA user_version = 1").unwrap();
        }
        let store = Store::open(&db).unwrap();
        let rows = store.list_sessions(10).unwrap();
        let row = rows
            .iter()
            .find(|r| r.device_name == "d1")
            .expect("老数据应保留");
        assert_eq!(row.account_key, "main", "account_id → account_key 后值不变");
        let v: i64 = Connection::open(&db)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn session_lifecycle_and_unique_active() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .create_session(crate::store::NewSession {
                account_key: "a",
                device_name: "d",
                executor: "mower",
                runner: Some("process"),
                state: "running",
                mower_port: Some(58100),
                slice_deadline_ms: None,
                max_runtime_deadline_ms: None,
            })
            .unwrap();
        assert_eq!(store.active_session_by_device("d").unwrap().unwrap().id, id);
        assert_eq!(
            store
                .active_session_by_account_key("a")
                .unwrap()
                .unwrap()
                .id,
            id
        );

        store
            .update_session_state(id, "draining", "drain", Some("slice 到期"))
            .unwrap();
        let ev = store.session_events(id).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].detail.as_deref(), Some("slice 到期"));

        store
            .finish_session(id, "finished", "completed", None, "finish")
            .unwrap();
        assert!(store.active_session_by_device("d").unwrap().is_none());
        let s = store.get_session(id).unwrap().unwrap();
        assert_eq!(s.state, "finished");
        assert!(s.ended_at_ms.is_some());
        assert!(store.get_session(999).unwrap().is_none());
    }

    #[test]
    fn lease_mutex_and_orphan_recovery() {
        let store = Store::open_in_memory().unwrap();
        store.acquire_lease("d1", "session:1").unwrap();
        let err = store.acquire_lease("d1", "switch:a").unwrap_err();
        assert!(
            matches!(err, StoreError::LeaseConflict { ref holder, .. } if holder == "session:1")
        );
        // 不同设备互不影响
        store.acquire_lease("d2", "switch:a").unwrap();
        // 持有者匹配才能释放
        assert!(!store.release_lease("d1", "switch:a").unwrap());
        assert!(store.release_lease("d1", "session:1").unwrap());
        assert!(store.lease_holder("d1").unwrap().is_none());

        // 孤儿回收：把心跳改旧
        store.acquire_lease("d3", "session:7").unwrap();
        store
            .conn()
            .execute(
                "UPDATE device_leases SET heartbeat_at_ms = 1 WHERE device_name='d3'",
                [],
            )
            .unwrap();
        let recovered = store
            .recover_orphan_leases(Duration::from_secs(60))
            .unwrap();
        assert_eq!(recovered, vec!["d3".to_string()]);
        store.acquire_lease("d3", "session:8").unwrap();
    }

    #[test]
    fn port_allocation_skips_used_and_probe_bind() {
        let store = Store::open_in_memory().unwrap();
        // 端口段运行时探测：硬编码 58100-58103 会与本机在跑的 mower/其它测试冲突
        // （该测试曾在原代码上 4/6 次失败）。这里取一段当前确实空闲的连续端口。
        let base = free_port_base(4).expect("找不到 4 段连续空闲端口");
        let range = [base, base + 3];

        // 预占段首端口（模拟已记账）
        store
            .conn()
            .execute(
                "INSERT INTO port_allocations(port, holder, kind, allocated_at_ms) VALUES (?1, 'other', 'mower_webview', 0)",
                [base as i64],
            )
            .unwrap();
        // 段次端口（base+1）被系统层占用
        let hold = std::net::TcpListener::bind(("127.0.0.1", base + 1)).unwrap();

        let p1 = store
            .alloc_port("mower_webview", "session:1", range)
            .unwrap();
        assert_eq!(p1, base + 2, "跳过记账段首与被绑的段次");
        let p2 = store
            .alloc_port("mower_webview", "session:2", range)
            .unwrap();
        assert_eq!(p2, base + 3);

        // 段耗尽（段首记账 / 段次被绑 / 段三段四已记账）
        let err = store
            .alloc_port("mower_webview", "session:3", range)
            .unwrap_err();
        assert!(matches!(err, StoreError::PortExhausted { .. }));

        drop(hold);
        // 释放后可复用：段次端口先被探测到
        assert_eq!(store.release_ports("session:1").unwrap(), 1);
        assert_eq!(
            store
                .alloc_port("mower_webview", "session:4", range)
                .unwrap(),
            base + 1
        );
    }

    /// 找一段连续 `n` 个当前可绑定的端口（跳过系统保留与在用端口）。
    fn free_port_base(n: u16) -> Option<u16> {
        for base in 49152..60000u16.saturating_sub(n) {
            if (base..base + n).all(|p| {
                std::net::TcpListener::bind(("127.0.0.1", p))
                    .map(drop)
                    .is_ok()
            }) {
                return Some(base);
            }
        }
        None
    }

    #[test]
    fn logins_upsert_keeps_first_at() {
        let store = Store::open_in_memory().unwrap();
        store.record_login("a", "d", "provisioned").unwrap();
        store
            .conn()
            .execute(
                "UPDATE logins SET first_at_ms = 1000 WHERE account_key='a'",
                [],
            )
            .unwrap();
        store.record_login("a", "d", "provisioned").unwrap();
        let first: i64 = store
            .conn()
            .query_row(
                "SELECT first_at_ms FROM logins WHERE account_key='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first, 1000, "upsert 不得改 first_at");
        assert_eq!(
            store.login_status("a", "d").unwrap().as_deref(),
            Some("provisioned")
        );
        assert_eq!(store.list_logins("a").unwrap().len(), 1);
        assert!(store.login_status("a", "nope").unwrap().is_none());
    }

    #[test]
    fn switch_log_and_maintenance_insert() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_switch_log(&SwitchLogEntry {
                ts_ms: now_ms(),
                account_key: "a".into(),
                device_name: "d".into(),
                ok: true,
                duration_ms: 91234,
                retries: 1,
                maa_log_excerpt: Some("..".into()),
            })
            .unwrap();
        store
            .insert_maintenance("maa", "update", Some("6.17.5"), Some("6.18.0"), true, None)
            .unwrap();
    }
}
