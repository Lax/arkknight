//! SQLite 持久化（§14）：rusqlite + WAL + bundled，`state/akops.db`。
//!
//! **M1 任务 3（租约）与任务 7（会话）交付实现。** INV-4：SQLite 仅存运行态
//! 与统计；账号/设备/策略以文件树为准；可由文件树 + 日志重建的部分不入库。
//!
//! Schema v1（建表语句，与设计文档 §14 一致；实现时以此为唯一迁移基线）：
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS sessions(
//!   id INTEGER PRIMARY KEY AUTOINCREMENT,
//!   account_id TEXT NOT NULL, device_name TEXT NOT NULL,
//!   executor TEXT NOT NULL, runner TEXT, state TEXT NOT NULL,
//!   mower_port INTEGER, slice_deadline TEXT, max_runtime_deadline TEXT,
//!   started_at TEXT, ended_at TEXT, outcome TEXT, error TEXT);
//! CREATE TABLE IF NOT EXISTS session_events(
//!   id INTEGER PRIMARY KEY AUTOINCREMENT, session_id INTEGER NOT NULL,
//!   ts TEXT NOT NULL, kind TEXT NOT NULL, detail_json TEXT);
//! CREATE TABLE IF NOT EXISTS device_leases(
//!   device_name TEXT PRIMARY KEY, session_id INTEGER NOT NULL,
//!   acquired_at TEXT NOT NULL, heartbeat_at TEXT NOT NULL);
//! CREATE TABLE IF NOT EXISTS port_allocations(
//!   port INTEGER PRIMARY KEY, session_id INTEGER NOT NULL, kind TEXT NOT NULL);
//! CREATE TABLE IF NOT EXISTS switch_log(
//!   id INTEGER PRIMARY KEY AUTOINCREMENT, ts TEXT NOT NULL,
//!   account_id TEXT NOT NULL, device_name TEXT NOT NULL,
//!   ok INTEGER NOT NULL, duration_ms INTEGER NOT NULL, retries INTEGER NOT NULL,
//!   maa_log_excerpt TEXT);
//! CREATE TABLE IF NOT EXISTS logins(
//!   account_id TEXT NOT NULL, device_name TEXT NOT NULL, status TEXT NOT NULL,
//!   first_at TEXT NOT NULL, last_verified_at TEXT NOT NULL,
//!   PRIMARY KEY(account_id, device_name));
//! CREATE TABLE IF NOT EXISTS maintenance_log(
//!   id INTEGER PRIMARY KEY AUTOINCREMENT, ts TEXT NOT NULL, target TEXT NOT NULL,
//!   action TEXT NOT NULL, from_v TEXT, to_v TEXT, ok INTEGER NOT NULL, detail TEXT);
//! CREATE TABLE IF NOT EXISTS stats_daily(
//!   day TEXT NOT NULL, account_id TEXT NOT NULL,
//!   minutes_run INTEGER NOT NULL DEFAULT 0, sessions_cnt INTEGER NOT NULL DEFAULT 0,
//!   switch_cnt INTEGER NOT NULL DEFAULT 0, sanity_spent INTEGER,
//!   PRIMARY KEY(day, account_id));
//! CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
//! ```
//!
//! 约束：`meta.schema_version` 版本化 + 内建迁移器；`akops migrate` 手动触发。
//! daemon 单写者 + standalone flock 互斥（ADR-0001 D7）由上层保证。
