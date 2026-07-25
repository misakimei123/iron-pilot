# IronPilot Development Progress

> 文档职责：当前实施进度与证据的唯一权威来源
>
> This file is the single source of truth for current implementation progress.
>
> Plan baseline: DEVELOPMENT_PLAN v2.2.0
>
> Plan commit: `000ccf8257f0fb5a1fa0417792710ff654ba1d56`
>
> Last updated: 2026-07-25

## Document Boundary

- 本文件不重新定义产品范围、Task 依赖、验收标准或阶段 Gate。
- 所有静态定义均引用 `docs/DEVELOPMENT_PLAN.md`。
- 本文件与开发计划冲突时，以 `docs/DEVELOPMENT_PLAN.md` 为准。
- 若发现开发计划本身需要改变，必须停止当前 Task 并申请显式计划修订，不得在本文件中改写计划。

## Current Focus

- Current phase: Phase B — Market to AI
- In progress: None
- Ready:
  - P2-02
  - P3-01
- Blocked: None
- Next recommended task: P2-02

## Task Status

| Task | Status | Started | Completed | Implementation Commit | Evidence | Notes |
|---|---|---|---|---|---|---|
| `P0-01` | `DONE` | — | 2026-07-24 | `edb5f861d2377b1ee9e95da934acffd226f1f938` | `CONTEXT.md`; `docs/adr/0001-spot-first-mvp.md` 至 `0004-compositional-historical-backtesting.md` | 历史架构基线 |
| `P0-02` | `DONE` | — | 2026-07-24 | `033de133b42ce9a66b126fd8376fdd60b5b34a77` | `docs/DEVELOPMENT_PLAN.md` v2 重建及后续一致性修订 | 仅完成规划治理，未实施业务 Task |
| `P0-03` | `DONE` | 2026-07-25 | 2026-07-25 | `98f87db43c9d8c23d563ce8df43d521ea434c924` | `docs/adr/0005-bounded-ai-strategy-authority.md`; ADR-0002/0003/0004 superseded/amended 标记；`CONTEXT.md`; 语义断言与 `git diff --check` | 未修改开发计划；未批准任何阶段 Gate |
| `P1-01` | `DONE` | 2026-07-25 | 2026-07-25 | `705ca6f7b5aa4602072cc943295c15ae66bb780e` | Rust 质量门禁；空进程 smoke test；cargo-deny；Gitleaks；CI YAML 校验；零第三方 Cargo 依赖断言 | 无业务伪实现；未修改开发计划或批准 Gate |
| `P1-02` | `DONE` | 2026-07-25 | 2026-07-25 | `a2d2f4a9ad2851cb9443606942f274e6fa16a914` | 精确 Decimal、稳定 ID、Instrument 与 Strategy Intent 契约测试；三组状态机属性测试；Rust 全门禁；cargo-deny；Gitleaks；无浮点领域类型断言 | 未修改开发计划；未批准任何阶段 Gate |
| `P1-03` | `DONE` | 2026-07-25 | 2026-07-25 | `62cda475f2d5d7d447264ad916130b3e8cddce9d` | 严格 YAML/环境加载；环境指纹与版本校验；1–3 个 Spot 标的；2C2G 上限；权限单调热加载；33 项测试；cargo-deny；Gitleaks | 未修改开发计划；未批准任何阶段 Gate |
| `P1-04` | `DONE` | 2026-07-25 | 2026-07-25 | `05dba297c7120d6e9e7fd01b06d3b3ad25c67413` | SQLx migration/WAL；关键状态、审计与 outbox 原子写；append-only 触发器；租约隔离与过期接管；备份完整性和恢复；6 项专项测试、39 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P1-05` | `DONE` | 2026-07-25 | 2026-07-25 | `24e7e87ea698e749c1ffad423136e36655ce3f31` | Tokio 有界任务监督与 watch 取消；1024/256 有界队列和 correlation ID；饱和/关闭不静默丢失；可信健康快照；RSS/CPU 采样；1400 MiB 软门槛降级；协作/强制 shutdown；5 项专项测试、44 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-01` | `DONE` | 2026-07-25 | 2026-07-25 | `ffab892dad1318633f6665dcbb39b14900fca10c` | Bybit fixtures、TTL/hash、错误分类、在线只读 smoke、52 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-02` | `READY` | — | — | — | — | `P2-01`,`P1-05` 已完成 |
| `P2-03` | `PLANNED` | — | — | — | — | — |
| `P2-04` | `PLANNED` | — | — | — | — | — |
| `P3-01` | `READY` | — | — | — | — | `P1-04`,`P2-01` 已完成 |
| `P3-02` | `PLANNED` | — | — | — | — | — |
| `P3-09` | `PLANNED` | — | — | — | — | — |
| `P3-03` | `PLANNED` | — | — | — | — | — |
| `P3-04` | `PLANNED` | — | — | — | — | — |
| `P3-05` | `PLANNED` | — | — | — | — | — |
| `P3-10A` | `PLANNED` | — | — | — | — | — |
| `P3-06` | `PLANNED` | — | — | — | — | — |
| `P3-07A` | `PLANNED` | — | — | — | — | — |
| `P3-08` | `PLANNED` | — | — | — | — | — |
| `P3-07B` | `PLANNED` | — | — | — | — | — |
| `P3-VS` | `PLANNED` | — | — | — | — | — |
| `P3-10B` | `PLANNED` | — | — | — | — | — |
| `P3-11` | `PLANNED` | — | — | — | — | — |
| `P4-01` | `PLANNED` | — | — | — | — | — |
| `P4-02A` | `PLANNED` | — | — | — | — | — |
| `P4-02B` | `PLANNED` | — | — | — | — | — |
| `P4-03` | `PLANNED` | — | — | — | — | — |
| `P4-04` | `PLANNED` | — | — | — | — | — |
| `D-NEWS-01` | `DEFERRED` | — | — | — | — | 不属于当前排期 |
| `P5-*` | `DEFERRED` | — | — | — | — | 永续合约；需重新授权 |
| `P6-*` | `DEFERRED` | — | — | — | — | 真实资金与扩容；需独立 Release Gate 与明确授权 |

## Active Blockers

None.

未来阻塞项必须记录：

- Task ID；
- 阻塞原因；
- 发现日期；
- 相关提交或日志；
- 解除条件；
- 是否需要修改开发计划。

## Recent Completions

### P0-01 — 历史架构基线

- 结果：建立 Spot-first 架构基线、项目上下文和 ADR-0001 至 ADR-0004。
- Commit：`edb5f861d2377b1ee9e95da934acffd226f1f938`。
- 证据：`CONTEXT.md`、`docs/DEVELOPMENT_PLAN.md` 初始基线和四份 ADR。
- 已知限制：属于历史规划基线，不代表任何业务实现、外部调用或 Gate 已完成。

### P0-02 — DEVELOPMENT_PLAN v2 权威计划重建

- 结果：建立 DEVELOPMENT_PLAN v2，并完成后续范围、依赖、权限和 Gate 一致性修订。
- Commit：`033de133b42ce9a66b126fd8376fdd60b5b34a77`。
- 证据：`docs/DEVELOPMENT_PLAN.md` v2.2.0；Plan baseline commit `000ccf8257f0fb5a1fa0417792710ff654ba1d56`。
- 已知限制：仅完成文档治理；没有开始 P0-03、P1-01 或任何业务开发 Task。

### P0-03 — ADR 与领域词汇对齐

- 结果：新增 ADR-0005，冻结有界 AI 策略权限；ADR-0002 标记为 superseded；ADR-0003/0004 与 `CONTEXT.md` 对齐 Eligibility / Event Prefilter、Strategy Intent、Deterministic Strategy Materialization 和无新闻默认链语言。
- Commit：`98f87db43c9d8c23d563ce8df43d521ea434c924`。
- 证据：`docs/adr/0005-bounded-ai-strategy-authority.md`；ADR 相对链接检查；P0-03 语义断言；`git diff --check`；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只完成文档与术语治理，不包含业务实现，不批准任何阶段 Gate，也不引入新闻能力。

### P1-01 — Rust 工程骨架与质量门禁

- 结果：建立 `ironpilot-domain`、`ironpilot-application`、`ironpilot-adapters` 和 `ironpilot` 四 crate 模块化单体骨架；固定 Rust 1.97.1；加入示例配置、CI、依赖治理、license/advisory/source policy 和 Git 历史秘密扫描。
- Commit：`705ca6f7b5aa4602072cc943295c15ae66bb780e`。
- 证据：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo test --workspace --all-targets --locked`（1 个 smoke test 通过）；`cargo build --workspace --all-targets --locked`；`cargo metadata` 四 crate、零第三方依赖和零 Cargo feature 断言；cargo-deny 0.19.4 的 advisories/bans/licenses/sources 全部通过；Gitleaks 8.30.1 的完整历史与工作区扫描通过；CI YAML 语法检查通过。
- 已知限制：空应用按设计不执行任何业务；配置键、领域类型、状态机和交易行为分别由后续 Task 实现；本 Task 不批准任何阶段 Gate。

### P1-02 — 核心领域、Strategy Intent 与状态机

- 结果：冻结精确 Decimal、Bybit Instrument、类型化稳定 ID、`StrategyIntent v2` 与 `strategy-space-v1-vs` 可执行子集，并实现 System、TradePlan、Order 三组 fail-closed 状态机。
- Commit：`a2d2f4a9ad2851cb9443606942f274e6fa16a914`。
- 证据：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo test --workspace --all-targets --locked`（领域契约 20 项全部通过）；`cargo build --workspace --all-targets --locked`；cargo-deny advisories/bans/licenses/sources 全部通过；Gitleaks 历史和工作区扫描通过；领域源码无 `f32`/`f64` 类型。
- 已知限制：只冻结 `P3-VS` 前最小可执行策略空间和领域状态迁移，不包含配置、持久化、物化、风险或执行实现；不批准任何阶段 Gate。

### P1-03 — 配置、多标的与启动校验

- 结果：建立 `ironpilot-config-v1` 严格 YAML Schema、部署环境身份与指纹核对、版本绑定、1–3 个 Spot 标的和第 6.4 节全部 2C2G 上限；进程在配置通过前不初始化后续副作用，热加载只允许权限和资源单调收紧。
- Commit：`62cda475f2d5d7d447264ad916130b3e8cddce9d`。
- 证据：配置契约与进程测试 13 项、全工作区 33 项测试通过；`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo build --workspace --all-targets --locked`；cargo-deny advisories/bans/licenses/sources 全部通过；Gitleaks 历史和工作区扫描通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只建立配置读取、验证和保守热加载合同，不实现持久化、运行时监督、交易所访问、Risk 或 Execution；不批准任何阶段 Gate。

### P1-04 — SQLite、审计与单实例锁

- 结果：建立 SQLx 嵌入式 migration、SQLite WAL/FULL 同步、最多 4 连接和单写串行化；实现租约所有者 fencing、恢复用系统状态 Repository、审计/outbox 原子事务、数据库级 append-only 审计保护，以及经完整性检查的 `VACUUM INTO` 备份原型。
- Commit：`05dba297c7120d6e9e7fd01b06d3b3ad25c67413`。
- 证据：6 项存储专项测试覆盖 migration/WAL、事务回滚、审计更新/删除拒绝、第二实例拒绝、租约过期接管和备份恢复；全工作区 39 项测试通过；`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo build --workspace --all-targets --locked`；`cargo metadata --locked`；cargo-deny advisories/bans/licenses/sources 全部通过；Gitleaks 8.30.1 历史与源码工作树扫描通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只提供 Vertical Slice 前持久化内核和核心表，不实现后续业务 Repository、运行时 supervisor、交易所访问、Risk 或 Execution；备份是本地原型，不替代后续长期运行的保留、轮换和恢复演练；不批准任何阶段 Gate。

### P1-05 — 可观测性与运行时监督

- 结果：实现 Tokio 有界任务 supervisor、`watch` 协作取消和超时强制收敛；行情/关键事件队列分别采用配置冻结的 1024/256 容量，溢出或关闭会返还原事件并进入明确的 degrade/halt；事件携带 `CorrelationId`；健康快照同时报告资源新鲜度、RSS、CPU、队列深度/高水位和监督任务数，资源样本缺失/过期或 RSS 超过 1400 MiB 时禁止新 AI 与新开仓。
- Commit：`24e7e87ea698e749c1ffad423136e36655ce3f31`。
- 证据：5 项运行时专项测试覆盖队列容量与饱和、可信健康、内存软门槛、当前进程 RSS/CPU、任务上限、关键任务失败、协作与强制 shutdown；全工作区 44 项测试通过；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked`、cargo-deny advisories/bans/licenses/sources 和 Gitleaks 8.30.1 历史扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 提供运行时监督与健康内核，不实现后续行情接入、交易逻辑或外部健康接口；不批准任何阶段 Gate。

### P2-01 — Bybit 公共 REST 元数据

- 结果：评估官方 SDK 后采用最薄 `reqwest` public adapter；实现 Bybit 服务器时间、1–3 个 Spot 标的交易状态与精确价格/数量/金额/动态价格限制规则；Bybit DTO 保持在 adapter 内部，输出为领域合同；动态规则携带版本化 SHA-256 hash、6 小时默认 TTL 和 24 小时硬上限；HTTP、`retCode`、超时、限流、访问拒绝、无效响应和合同违规均有保守分类。
- Commit：`ffab892dad1318633f6665dcbb39b14900fca10c`。
- 证据：8 项 P2-01 fixture/合同测试覆盖服务器时间整数精度、精确十进制、Spot 无分页 cursor、规则 hash 顺序/表示规范化、TTL 过期边界、限流分类、请求上限和 HTTPS origin；全工作区 52 项测试通过；Bybit 主网公共只读 `server time` 与 `BTCUSDT` instruments smoke 返回 `retCode=0`、`Trading` 和预期精度字段；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 8.30.1 历史与源码工作树扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只实现公共 REST 元数据，不包含 WebSocket、私有接口、自动重试、持久化缓存、交易逻辑或执行权限；调用方必须在 TTL 到期前刷新或按过期规则 fail closed；不批准任何阶段 Gate。

## Next Action

Execute P2-02 next.

以上内容是依据 `docs/DEVELOPMENT_PLAN.md` 静态依赖生成的进度建议，不改变任何 Task 依赖。

## Status Update Rules

### Task 开始时

只更新本文件：

```text
READY → IN_PROGRESS
Started → 实际开始时间
Current Focus → 当前 Task
```

### Task 完成时

只有全部验收通过后：

```text
IN_PROGRESS → DONE
Completed → 实际完成时间
Implementation Commit → 准确 SHA
Evidence → 测试或证据文档引用
```

### Task 失败或受阻时

```text
IN_PROGRESS → BLOCKED
```

同时记录阻塞证据和解除条件。

### Task 依赖满足时

根据 `docs/DEVELOPMENT_PLAN.md` 的静态依赖更新后继 Task：

```text
PLANNED → READY
```

不得自行改变依赖关系。

## Prohibited Changes

本文件不得：

- 修改 Task 范围；
- 修改直接依赖；
- 修改验收标准；
- 修改 Gate；
- 引入新能力；
- 宣布真实资金授权；
- 宣布 Testnet 写授权；
- 把未验证 Task 标为 `DONE`；
- 仅凭代码已提交就把 Task 标为 `DONE`；
- 使用本文件覆盖 `docs/DEVELOPMENT_PLAN.md`。
