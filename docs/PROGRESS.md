# IronPilot Development Progress

> 文档职责：当前实施进度与证据的唯一权威来源
>
> This file is the single source of truth for current implementation progress.
>
> Plan baseline: DEVELOPMENT_PLAN v3.0.0
>
> Plan commit: `93ba9018a50bb49a215eca07e387552d51791a86`
>
> Last updated: 2026-07-25

## Document Boundary

- 本文件不重新定义产品范围、Task 依赖、验收标准或阶段 Gate。
- 所有静态定义均引用 `docs/DEVELOPMENT_PLAN.md`。
- 本文件与开发计划冲突时，以 `docs/DEVELOPMENT_PLAN.md` 为准。
- 若发现开发计划本身需要改变，必须停止当前 Task 并申请显式计划修订，不得在本文件中改写计划。

## Current Focus

- Current phase: Phase C — AI-Dominant Paper
- In progress: None
- Ready:
  - P3-03
  - P3-13
- Blocked: None
- Next recommended task: P3-03

## Task Status

| Task | Status | Started | Completed | Implementation Commit | Evidence | Notes |
|---|---|---|---|---|---|---|
| `P0-01` | `DONE` | — | 2026-07-24 | `edb5f861d2377b1ee9e95da934acffd226f1f938` | `CONTEXT.md`; `docs/adr/0001-spot-first-mvp.md` 至 `0004-compositional-historical-backtesting.md` | 历史架构基线 |
| `P0-02` | `DONE` | — | 2026-07-24 | `033de133b42ce9a66b126fd8376fdd60b5b34a77` | `docs/DEVELOPMENT_PLAN.md` v2 重建及后续一致性修订 | 仅完成规划治理，未实施业务 Task |
| `P0-03` | `DONE` | 2026-07-25 | 2026-07-25 | `98f87db43c9d8c23d563ce8df43d521ea434c924` | `docs/adr/0005-bounded-ai-strategy-authority.md`; ADR-0002/0003/0004 superseded/amended 标记；`CONTEXT.md`; 语义断言与 `git diff --check` | 未修改开发计划；未批准任何阶段 Gate |
| `P0-04` | `DONE` | 2026-07-25 | 2026-07-25 | `93ba9018a50bb49a215eca07e387552d51791a86` | DEVELOPMENT_PLAN v3.0.0；ADR-0006；Task 表/依赖图/正文/Gate 一致性；AITradingPlan JSON、ADR 链接与 v2 遗留隔离检查 | 独立计划修订，无业务代码；未批准任何阶段 Gate |
| `P1-01` | `DONE` | 2026-07-25 | 2026-07-25 | `705ca6f7b5aa4602072cc943295c15ae66bb780e` | Rust 质量门禁；空进程 smoke test；cargo-deny；Gitleaks；CI YAML 校验；零第三方 Cargo 依赖断言 | 无业务伪实现；未修改开发计划或批准 Gate |
| `P1-02` | `DONE` | 2026-07-25 | 2026-07-25 | `a2d2f4a9ad2851cb9443606942f274e6fa16a914` | 精确 Decimal、稳定 ID、Instrument 与 Strategy Intent 契约测试；三组状态机属性测试；Rust 全门禁；cargo-deny；Gitleaks；无浮点领域类型断言 | 未修改开发计划；未批准任何阶段 Gate |
| `P1-03` | `DONE` | 2026-07-25 | 2026-07-25 | `62cda475f2d5d7d447264ad916130b3e8cddce9d` | 严格 YAML/环境加载；环境指纹与版本校验；1–3 个 Spot 标的；2C2G 上限；权限单调热加载；33 项测试；cargo-deny；Gitleaks | 未修改开发计划；未批准任何阶段 Gate |
| `P1-04` | `DONE` | 2026-07-25 | 2026-07-25 | `05dba297c7120d6e9e7fd01b06d3b3ad25c67413` | SQLx migration/WAL；关键状态、审计与 outbox 原子写；append-only 触发器；租约隔离与过期接管；备份完整性和恢复；6 项专项测试、39 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P1-05` | `DONE` | 2026-07-25 | 2026-07-25 | `24e7e87ea698e749c1ffad423136e36655ce3f31` | Tokio 有界任务监督与 watch 取消；1024/256 有界队列和 correlation ID；饱和/关闭不静默丢失；可信健康快照；RSS/CPU 采样；1400 MiB 软门槛降级；协作/强制 shutdown；5 项专项测试、44 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-01` | `DONE` | 2026-07-25 | 2026-07-25 | `ffab892dad1318633f6665dcbb39b14900fca10c` | Bybit fixtures、TTL/hash、错误分类、在线只读 smoke、52 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-02` | `DONE` | 2026-07-25 | 2026-07-25 | `8bc805aed916df1a56ef4472484ad3bfc5ed1702` | 1–3 标的确定性订阅与重订阅；heartbeat、去重、乱序、重连、freshness 和显式 backpressure；8 项专项测试、60 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-03` | `DONE` | 2026-07-25 | 2026-07-25 | `632cc6f82c1b2f0c9523ffe4a08b8522491f69a7` | `ironpilot-market-features-v1`、双周期完整性、实时价差、稳定 snapshot/event hash、可解释 Prefilter、TTL/去重/冷却/预算；13 项专项测试、73 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P2-04` | `DONE` | 2026-07-25 | 2026-07-25 | `67ab2afefc034022a853755a1914094147730bbb` | `ironpilot-market-replay-v1` manifest/dataset/report hash、固定 clock/seed、`strategy-space-v1-vs` 绑定、future-data 隔离；9 项专项测试、82 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P3-01` | `DONE` | 2026-07-25 | 2026-07-25 | `156adcc66c8b1cead0f2619d9d92e203759986ab` | `ironpilot-portfolio-v1`、受管数量卖出边界、余额差异阻止新开仓、Fill/ManagedLot 原子幂等与对账审计；10 项专项测试、92 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P3-02` | `DONE` | 2026-07-25 | 2026-07-25 | `be3bb43855d3b92398c965203cade3e199e08c6b` | v2 `ironpilot-risk-rules-v1` 历史实现与原验收证据 | v3 遗留，不得进入活动链；已由 P3-12 安全退役，历史证据保留 |
| `P3-09` | `CANCELLED` | — | — | — | DEVELOPMENT_PLAN v3.0.0；ADR-0006 | 未开始实现；Materializer 与 AI 主导交易权限冲突 |
| `P3-12` | `DONE` | 2026-07-25 | 2026-07-25 | `117dad5dede912b3850b93ff8bf47404bde32a84` | `AITradingPlan v3` 合同；v2 源码/API/状态/配置隔离；Replay v2 版本证据；遗留 SQLite 表只读封存；8 项专项测试与全量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P3-03` | `READY` | — | — | — | — | P3-12 完成后依赖已满足 |
| `P3-04` | `PLANNED` | — | — | — | — | — |
| `P3-13` | `READY` | — | — | — | — | P3-12 完成后依赖已满足 |
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

### P0-04 — DEVELOPMENT_PLAN v3 AI 主导架构修订

- 结果：依据用户明确确认，将权威主链修订为完整 Market/Account Context → DeepSeek `AITradingPlan v3` → 只接受或拒绝的 Execution Validation/User Authorization → 忠实 TradePlan/Execution；取消活动链中的 Strategy Space 白名单、确定性 Materializer 和后置策略型 Risk Engine；AI 直接决定精确 entry、quantity、stop、take-profit 和正常持仓管理。
- Commit：`93ba9018a50bb49a215eca07e387552d51791a86`。
- 证据：DEVELOPMENT_PLAN 版本提升至 3.0.0；新增 ADR-0006 并 supersede ADR-0005；同步 ADR-0002/0003/0004、`CONTEXT.md` 和 v2 历史文档标记；35 个 Task 定义、32 个活动图节点、全部直接依赖逐项一致；`AITradingPlan v3` JSON 可解析；Task 引用、ADR 相对链接、遗留 P3-02/P3-09 隔离和 `git diff --check` 通过。
- 已知限制：本 Task 只完成计划、ADR、词汇和历史边界修订，当时没有删除 v2 代码或数据库结构，也没有实现 `AITradingPlan v3`；其后由 P3-12 完成安全迁移；未批准任何阶段 Gate。

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

### P2-02 — 多标的公共 WebSocket

- 结果：实现 Bybit Spot 公共 WebSocket adapter；为 1–3 个标的生成确定性的 15 分钟 K 线、60 分钟 K 线和一级 orderbook 订阅集合；支持应用 heartbeat、指数退避重连、原集合重订阅、K 线与 orderbook 去重/乱序防护、服务重启快照处理、每标的/主题 freshness，以及接入现有 1024 容量行情队列的显式 backpressure；消息、帧和写缓冲均设置硬上限。
- Commit：`8bc805aed916df1a56ef4472484ad3bfc5ed1702`。
- 证据：8 项 P2-02 专项测试覆盖确定性订阅集合、精确 Decimal 映射、K 线与 orderbook 去重/乱序、每主题 freshness、订阅确认/拒绝、队列饱和、Spot-only 配置与退避上限，以及本地真实 WebSocket 断线重连、原集合重订阅和恢复后交付；全工作区 60 项测试通过，1 项依赖 Bybit 线上 WSS 的只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：当前执行环境无法完成 Bybit 主网 WSS 握手，因此未把线上只读 smoke 计为通过；协议恢复和有界性验收由确定性测试及本地真实 WebSocket 端到端测试证明。本 Task 不包含私有流、交易写操作、市场特征、历史回放或任何 Gate 批准。

### P2-03 — Market Features 与 Eligibility/Event Engine

- 结果：冻结并实现独立的 `ironpilot-market-features-v1` 合同；从 120 根连续已闭合 15m/1h K 线和 30 秒内一级盘口生成 Donchian、EMA、Wilder RSI/ATR/ADX、成交量比率、EMA alignment、关键位置、11 种受控形态和实时价差；输入与输出分别形成规范化 SHA-256，REST bootstrap、WebSocket live 和 replay 的相同市场事实产生相同 snapshot hash；Eligibility/Event Prefilter 对数据 TTL、系统/标的/活动 TradePlan 状态、流动性、价差、信息增量、去重、冷却、并发、调用、Token 和成本预算给出稳定原因码，事件状态和去重表均有硬上限。
- Commit：`632cc6f82c1b2f0c9523ffe4a08b8522491f69a7`。
- 证据：13 项 P2-03 专项测试覆盖冻结指标已知向量、双周期对齐、future/stale/gap/duplicate/unclosed、11 种形态与冲突优先级、扁平动量 fail-closed、WS 输入到规范 candle/book 映射、跨传输与重启 hash 等价、event TTL、去重、冷却、无信息增量、状态解释、预算耗尽及 1024 项去重状态上限；全工作区 73 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只产生只读市场事实和是否允许后续 AI 决策尝试的稀疏事件，不选择方向、策略家族、入场、止损、目标、数量或执行政策；不包含 AI Provider、历史回放、持久化事件账本、交易写操作或任何 Gate 批准。

### P2-04 — 历史回放与可复现快照

- 结果：实现纯领域 `ironpilot-market-replay-v1` 回放合同；不可变 dataset hash 覆盖 1–3 个标的的连续 15m/1h 已闭合 K 线和有序一级盘口，manifest hash 绑定特征版本、`strategy-space-v1-vs`、固定种子、时钟范围与标的集合；固定 15 分钟 replay clock 在每个时点仅向既有 Feature/Eligibility Engine 暴露当时可见数据，并输出稳定的 Snapshot/Event 或拒绝原因报告 hash。
- Commit：`67ab2afefc034022a853755a1914094147730bbb`。
- 证据：9 项 P2-04 专项测试覆盖时钟对齐、同 manifest 两次 report/output hash 完全一致、固定 Strategy Space/种子绑定、future candle/book 隔离、dataset hash 不匹配失败、合法 JSON 且无新闻依赖或绩效结论字段、跨标的规范排序、数据边界/顺序 fail-closed 及复用 Feature Engine 的 warm-up 门槛；全工作区 82 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只复现市场 Snapshot 与 Eligibility Event，不执行订单、不生成持仓或绩效评估，也不实现后续 Minimal Historical Harness；不批准任何阶段 Gate。

### P3-01 — Portfolio、受管资产与对账

- 结果：实现 `ironpilot-portfolio-v1` 纯领域合同，以 P2-01 已验证 Spot Instrument Rules 绑定 Fill 的标的及 base/quote 资产；Portfolio Snapshot 明确区分交易所 available/locked/total、本地预期、可证明受管数量、未知盈余和短缺并生成稳定 hash，任何余额差异均禁止新开仓；卖出授权不能超过受管数量或交易所可用数量。SQLite 在有效单实例租约下原子写入 Fill、按 `(opened_at, managed_lot_id)` 消耗 ManagedLot 和追加审计，重复相同 Fill/对账 Run 业务效果为 0，幂等键内容冲突和超量卖出 fail closed。
- Commit：`156adcc66c8b1cead0f2619d9d92e203759986ab`。
- 证据：10 项 P3-01 专项测试覆盖 Instrument Rules 资产绑定、买卖 Fill 合同、受管/可用数量卖出上限、未知资产与短缺分类、任意余额差异阻止新开仓、规范快照 hash、重复/非法资产 fail-closed、买卖 Fill 持久化幂等、幂等键内容冲突、超量卖出事务回滚及对账/审计原子幂等；全工作区 92 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 接受上游提供的交易所余额事实但不调用私有 API；不创建订单、不模拟成交、不计算手续费、不裁决 Risk，也不实现 TradePlan 生命周期或任何阶段 Gate 批准。

### P3-02 — v2 确定性 Risk Engine（历史）

- 结果：实现 `ironpilot-risk-rules-v1` 纯领域合同；Risk 输入必须绑定本地已验证的 `strategy-space-v1-vs` Intent、原始 decision/snapshot/instrument/action、物化算法版本与不可变 hash。裁决结果封闭为 `APPROVE`、`ADJUST_DOWN`、`REJECT`、`REDUCE_ONLY`、`HALT_SYMBOL`、`HALT_SYSTEM`；只有批准或向下调整能产生私有构造的 `RiskAuthorization`，并保留原策略身份且数量永不增加。Portfolio 差异、活动 TradePlan 上限、系统/标的降权及硬上限破坏均 fail closed；决策 hash 绑定全部裁决输入、上下文、结果和原因。
- Commit：`be3bb43855d3b92398c965203cade3e199e08c6b`。
- 证据：10 项 P3-02 专项测试覆盖合法追溯、只降不升、零额度、Portfolio 差异、活动计划上限与硬上限破坏、系统/标的降权、非 `strategy-space-v1-vs`/非 `OPEN_LONG` 输入拒绝、六种结果封闭、确定性 hash 及授权数量属性测试；全工作区 102 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与本次源码工作树扫描、`git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本段结果与证据属于 v2 历史。DEVELOPMENT_PLAN v3.0.0 和 ADR-0006 已禁止该 Risk Engine 进入活动链；P3-12 已安全退役其活动代码/API并只读封存遗留 Schema，历史审计继续保留。本 Task 不批准任何阶段 Gate。

### P3-12 — AITradingPlan v3 合同与 v2 权限迁移

- 结果：冻结严格 `AITradingPlan v3` 领域合同，AI 可直接表达 `OPEN_LONG`、`NO_TRADE`、`HOLD`、`CANCEL_ENTRY`、`MODIFY_PROTECTION`、`REDUCE` 与 `EXIT`，并原生携带精确 order/quantity/stop/take-profit/validity/management、声明最大亏损、复评和叙事；全部数值使用精确十进制字符串，未知字段、浮点、非法单位、非 Spot、`OPEN_SHORT` 和动作字段错配 fail closed；计划生成稳定 canonical hash，活动领域不提供本地策略构造器。
- 迁移：将 v2 Strategy Space、Materializer 与确定性 Risk Engine 源码/测试移入非编译 `legacy/v2` 历史目录并移出公共 API；TradePlan 状态删除 `MATERIALIZED`/`RISK_APPROVED`，改为 `PROPOSED → ACCEPTED`；配置升级为 `ironpilot-config-v2`，绑定 AI Decision Context 与 AITradingPlan 版本；Replay 升级为 v2 manifest/report 并用 Context/Plan 版本证据替代 Strategy Space；SQLite 保留三个 v2 表，但以 9 个触发器禁止新增、修改和删除。
- Commit：`117dad5dede912b3850b93ff8bf47404bde32a84`。
- 证据：7 项 `AITradingPlan v3` 合同测试和 1 项遗留表封存测试覆盖完整方案 roundtrip、7 个动作、精确参数与稳定 hash、未知字段、浮点/单位、Spot 非法方向、动作字段、v2 输入隔离、活动 API 无旧权限模型、TradePlan 旧状态拒绝、Replay v3 版本绑定及数据库写入封存；全工作区 92 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描、v3 活动权限/Replay/遗留证据静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只建立 AI 方案合同和安全迁移边界，不构建完整 Decision Context/TradePlan Ledger、不调用 DeepSeek、不执行 Validator 或订单；分别由 P3-03、P3-04、P3-13 和 P3-05 完成。本 Task 不批准任何阶段 Gate。

## Next Action

Execute P3-03 next. P3-13 is also READY and remains independent until its downstream dependencies converge.

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
