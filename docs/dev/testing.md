# 测试策略

设计文档 §19 是策略总纲，本文是可直接照做的操作手册。

## 跑测试

```bash
cargo test --workspace                      # 全量（单测 + golden + 调度场景）
cargo test -p arkknight-core --lib         # 只跑库单测
cargo test -p arkknight-core --test golden # 只跑物化器 golden
cd ui && npm run build                      # 前端类型检查 + 构建（vue-tsc）
```

## 质量门禁（提交前必过）

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings   # 零告警是硬约定
cargo test --workspace
cd ui && npm run build                     # 含 vue-tsc 类型检查
```

## 测试分层

| 层 | 位置 | 覆盖什么 |
|---|---|---|
| 单元（内联） | 各模块 `#[cfg(test)] mod tests` | 纯函数、校验、物化、状态机 |
| golden | `crates/arkknight-core/tests/golden.rs` + `tests/golden/` | 物化器产物快照 |
| 调度场景 | `crates/arkknight-core/tests/scheduler_scenarios.rs` | 引擎全场景（FakeExecutor） |
| API 契约 | `crates/arkknight/src/commands/server.rs` 的 `mod tests` | Router + 鉴权 + 错误 hint |

## golden-file 快照

物化器（MAA profile / tasks、mower conf patch）的输出**必须**有快照比对。

快照文件在 `crates/arkknight-core/tests/golden/`：

```
mower_conf_in.yaml     # 输入（含刻意设置的干扰项）
mower_conf_out.yaml    # 期望输出（只应改白名单字段）
profile_process.posix.toml
profile_docker.posix.toml
startup_task.toml
```

**改了物化器逻辑后更新快照：**

```bash
ARKKNIGHT_UPDATE_GOLDEN=1 cargo test -p arkknight-core --test golden
git diff crates/arkknight-core/tests/golden/     # 人工确认改动符合预期
```

改了不更新 → 测试失败，这是**故意的**。

golden 同时是「白名单改写没越界」的证明：对比 in/out 能看出只动了
`adb` / `webview.port` / `webview.token` / `start_automatically`。

## 调度场景测试（INV-3 的红利）

调度器只面向 `Executor` trait 编程，所以**不需要真设备**就能跑全场景。

`scheduler_scenarios.rs` 用两个假件：

- **`FakeExecutor`** —— `health` 由共享 `AtomicU8` 决定（0=Alive，1=Dead），`drain` 立即成功
- **假 maa 脚本** —— `exit 0/1` 控制切号成败，写进临时目录当 `maa_bin`

时间片/看门狗间隔压到 1s 级（`HumanDuration` 最小单位是秒）。

现有五个场景：

| 场景 | 验证什么 |
|---|---|
| `rotation_two_accounts_alternate_and_finish` | 两账号按优先级轮转，会话正常收尾 |
| `switch_failure_records_and_limits_rate` | 切号失败记 `switch_log` 并触发退避限流 |
| `watchdog_dead_executor_terminates_session` | 看门狗发现执行器死亡 → 终止会话 |
| `pause_prevents_new_sessions` | 暂停后不再起新会话 |
| `shutdown_drains_active_session` | SIGINT/SIGTERM 关停时优雅 drain 活跃会话（outcome=`cancelled`） |

**加新调度行为时，在 `scheduler_scenarios.rs` 加场景**，而不是写需要设备的测试。

## API 契约测试

`server.rs` 的 `mod tests` 用 `tower::ServiceExt::oneshot` 直接打 Router，
不起 HTTP server。

`tower` 是 **dev-dependencies**（不进发布二进制）。

现有覆盖：

| 测试 | 断言 |
|---|---|
| `静态资源不经鉴权_控制台可加载` | token 非空时 `/assets/*` **不得** 401（白屏回归防线） |
| `api_无token被拒_带token放行` | API 仍严格校验，`?token=` 与 `Bearer` 两条路都通 |
| `鉴权失败报错带hint与配置路径` | 401 带 `detail`/`hint`/`config_path`，hint 指向 `[server].token` |
| `不存在实体的报错列出已注册项` | 404 的 hint 含 `account add` 命令与已注册清单 |
| `创建账号可带uid` | uid 落盘、展示名回落 key、非数字被拒 |

**新增/修改路由时同步补测试。**

构造测试 state 的辅助函数在同文件的 `state_with_token()`：用
`Store::open_in_memory()` + `EngineHandle::new()` + 一个总是返回 `Err` 的
`ExecutorFactory`。

## 避免 flaky

**端口相关测试必须运行时探测空闲端口**，不能硬编码。

反面教材（已修）：`port_allocation_skips_used_and_probe_bind` 曾硬编码
58100-58103，与本机在跑的 mower 端口段冲突，**在原始代码上跑 6 次失败 4 次**。
现在用 `free_port_base(n)` 在 49152+ 扫一段连续可绑定的端口。

其它已知 flaky 来源：

- 依赖固定时间片/看门狗间隔 → 压到 1s 级，别靠 sleep 等真实时长
- 依赖本机网络/端口状态 → 运行时探测
- 共享 `process::id()` 做临时目录名 → 同进程并行测试会撞，用 `tempfile::tempdir()`

## 需要真实环境的部分

以下**无法在 CI 覆盖**，需人工验证（本机 `/srv/reunion` 部署栈）：

| 项 | 验证方式 |
|---|---|
| 切号真的切对了账号 | `arkknight switch <key>` + 人工确认游戏内账号 |
| UID 核验 pipeline 的 roi 校准 | 不同分辨率下实机校准 `render_uid_check_pipeline` |
| mower 深链 UI 改动持久化 | 会话中改 conf/plan，下次会话确认还在 |
| 72h 无人值守轮转 | M1 总验收 |

设计上已尽量隔离：`maa run --dry-run` 可在无设备时校验物化配置；
adb/maa 路径在 `SwitchCtx`/`EngineShared` 里可注入，测试用假脚本。

## CI

`.github/workflows/ci.yml`：fmt + clippy(deny warnings) + test（三平台矩阵）+ 前端 build。

`ci.yml` 与 `release.yml` 独立：CI 每次 push 跑，release 只在 tag `v*` 时构建六目标安装包。