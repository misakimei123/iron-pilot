# IronPilot Development Progress

> 文档职责：当前实施进度与证据的唯一权威来源
>
> This file is the single source of truth for current implementation progress.
>
> Plan baseline: DEVELOPMENT_PLAN v3.2.0
>
> Plan commit: `90d01665939456e5a68210be16187ddec927d14f`
>
> Last updated: 2026-07-26

## Document Boundary

- 本文件不重新定义产品范围、Task 依赖、验收标准或阶段 Gate。
- 所有静态定义均引用 `docs/DEVELOPMENT_PLAN.md`。
- 本文件与开发计划冲突时，以 `docs/DEVELOPMENT_PLAN.md` 为准。
- 若发现开发计划本身需要改变，必须停止当前 Task 并申请显式计划修订，不得在本文件中改写计划。

## Current Focus

- Current phase: Phase E — Parallel Hardening
- In progress: None
- Ready:
  - P3-11
  - P4-01
- Blocked: None
- Next recommended task: P3-11 Long-running Paper Safety; P4-01 remains independently READY

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
| `P3-03` | `DONE` | 2026-07-25 | 2026-07-25 | `e093bc8c11d7a9927450b6be652eb26eab4c2dd8` | 完整事实 Context/稳定 hash；原始响应与 AI Plan provenance；原子 TradePlan Ledger；8 项专项测试与全量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P3-04` | `DONE` | 2026-07-25 | 2026-07-25 | `98f30dddf68ab8fc88c522d9a24e1c2c123da3c7`；`7928509ddad5ff65e8ab4e8540db86b0d6cc00c6` | Prompt v1；`async-openai` DeepSeek client；strict parse；usage/cost/latency；预算；一次 replan；原始证据；13 项专项测试与全量门禁 | v3.1.0 开源优先重构；未批准任何阶段 Gate；确定性门禁不调用线上 DeepSeek |
| `P3-13` | `DONE` | 2026-07-26 | 2026-07-26 | `86203e92147acfd5671b158fd1edf546685a9347` | `ironpilot-execution-validator-v1`；完整兼容性/授权/最大亏损校验；原子 validation ledger；8 项专项持久化与领域测试；121 项全工作区测试及全部质量门禁 | 只返回 ACCEPT/REJECT，未修改 AI 方案或生成订单；未批准任何阶段 Gate |
| `P3-05` | `DONE` | 2026-07-26 | 2026-07-26 | `e05918fc57a3b51f9b94d6d42cc1a9b05e1ab7b9` | `ironpilot-spot-execution-v1`；共享 port；精确订单映射；部分成交/费用/滑点/保护单/ManagedLot；4 项专项测试；125 项全工作区测试及全部质量门禁 | 未修改开发计划；未批准任何阶段 Gate |
| `P3-10A` | `DONE` | 2026-07-26 | 2026-07-26 | `5f8b097220e94caed14ba74d1513410819370e14` | `ironpilot-minimal-historical-harness-v1`；确定性/前缀不变/无前视 2 项专项测试；127 项全工作区测试及全部质量门禁 | 不调用实时 LLM，不包含 Materializer/Risk Engine，不构建完整绩效平台；未批准任何阶段 Gate |
| `P3-06` | `DONE` | 2026-07-26 | 2026-07-26 | `f5a7bc061b06b1be471ab57eda740595d7f361d6` | `ironpilot-ai-paper-runtime-v1`；Runtime Prompt v2；append-only cycle trace；6 项专项测试；133 项全工作区测试及全部质量门禁 | 本地生成或改写交易参数次数为 0；主链无新闻、Materializer 或策略型 Risk Engine；未批准任何阶段 Gate |
| `P3-07A` | `DONE` | 2026-07-26 | 2026-07-26 | `290512c6ce0fdcb1119e3f847f654eeed0bbec00`；`2eccccea75d68647eadce3291d49d005ff393c8d` | `ironpilot-telegram-readonly-v1`；`teloxide-core 0.13.0` SDK；完整只读查询面；4 项专项测试；137 项全工作区测试及全部质量门禁 | DEVELOPMENT_PLAN v3.2.0 开源 SDK 强制复用纠正；生产路径 SQL 写入为 0；无策略或紧急控制命令；未批准任何阶段 Gate |
| `P3-08` | `DONE` | 2026-07-26 | 2026-07-26 | `a4545a2e527744298565a58bef6c320a9f3ced70` | `ironpilot-emergency-core-v1`；统一授权命令、5 分钟 TTL、幂等 hash；项目归属订单撤销；受管仓位部分减仓、重启恢复与 append-only 证据；4 项专项测试；141 项全工作区测试及全部质量门禁 | 完成后保持 `HALTED` 且不自动恢复入场；无 AI/Telegram 依赖；未知资产卖出为 0；未批准任何阶段 Gate |
| `P3-07B` | `DONE` | 2026-07-26 | 2026-07-26 | `1c68737b78669e9c4cd42c129f465e1eceaf025b` | `ironpilot-telegram-emergency-v1`；SDK chat/user 身份、UUID v4 nonce、二次确认、TTL、一次性消费与统一 Emergency Command；6 项 Telegram 专项测试；143 项全工作区测试及全部质量门禁 | 只构造授权命令，直接交易写入为 0；重启使未确认 challenge fail closed；未批准任何阶段 Gate |
| `P3-VS` | `DONE` | 2026-07-26 | 2026-07-26 | `919d77b0d67573a0a60729bae12fbf2f16ac3d72` | Repository evidence matrix；143 项测试；用户于 2026-07-26 明确接受证据并批准 Gate | 用户明确决定先不执行线上 DeepSeek/Telegram smoke；不授权 Testnet 写、实盘、永续或新闻能力 |
| `P3-10B` | `DONE` | 2026-07-26 | 2026-07-26 | `d4f732624574758a8972674ac7e19b9a5eef4860` | `docs/FULL_HISTORICAL_STRATEGY_EVALUATION_V1.md`; 3 targeted tests; 146 workspace tests | Offline-only evaluation; Rule-only is not production eligible; no stage Gate approved |
| `P3-11` | `READY` | — | — | — | — | P3-VS 已由用户批准 |
| `P4-01` | `READY` | — | — | — | — | P3-VS、P2-02 与 P3-01 均已完成 |
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
- 已知限制：本 Task 只建立 AI 方案合同和安全迁移边界，当时不构建完整 Decision Context/TradePlan Ledger、不调用 DeepSeek、不执行 Validator 或订单；Context/Ledger 和 DeepSeek Provider 其后分别由 P3-03、P3-04 完成，Validator 与订单分别由 P3-13、P3-05 完成。本 Task 不批准任何阶段 Gate。

### P3-03 — AI Decision Context 与 TradePlan Ledger

- 结果：实现 `ironpilot-ai-decision-context-v1` 不可变事实合同，固化完整 15m/1h 已闭合 K 线窗口、全部指标/形态、一级盘口、目标 Spot instrument rules、交易所时间与规则 hash、完整 Portfolio 资产、受管持仓、活动订单、用户最大亏损授权、版本/TTL 和 canonical SHA-256；构造时使用原始 K 线与盘口重新计算 `MarketFeatureSnapshot`，任何不一致、不完整窗口、未来/陈旧行情、未来/过期规则、未来账户/订单、重复持仓/订单或非法授权均 fail closed；Context JSON 不包含 action、recommendation、Strategy Space、Eligibility 方向、risk tier、anchor 或本地交易参数。
- 账本：实现 provider-neutral `AiRawResponse` 和 `AiTradePlanLedgerEntry`，逐项绑定 Context/response/AI plan/TradePlan/action ID 与三层 hash；`OPEN_LONG` 创建 `PROPOSED` TradePlan，`NO_TRADE` 创建终态追溯记录，其余管理动作必须追加到 AI 指定的现有同标的活动计划。SQLite migration 新增 `ai_decision_contexts`、`ai_provider_responses`、`ai_trading_plans` 和 `ai_trade_plan_ledger`；有效单实例租约下以单事务写入 Context、原始响应、解析计划、TradePlan/action、provenance link 与审计，重复相同内容业务效果为 0，ID 内容冲突、第二活动计划、目标计划不可用或审计失败全部回滚；任一 action 可查询回原始 Context/response/plan 及 hash。
- Commit：`e093bc8c11d7a9927450b6be652eb26eab4c2dd8`。
- 证据：5 项领域合同测试与 3 项 SQLite 账本测试覆盖完整事实序列、指标/形态/盘口/账户/持仓/订单/rules/最大亏损、输入顺序无关 hash、无本地推荐字段、future-data 与不可复现 Features 拒绝、跨 Context/stale provenance 拒绝、OPEN_LONG/HOLD 追溯、重复零副作用、每标的单活动计划、候选与审计失败完整回滚；全工作区 100 项测试通过，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与源码工作树扫描、Context 无本地策略/原子账本 Schema/迁移保留静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 当时不构建 DeepSeek request/usage/cost/latency、不执行 `ACCEPT/REJECT`、不生成 OrderIntent 或 Paper 订单；DeepSeek Provider 其后由 P3-04 完成，其余分别由 P3-13 和 P3-05 完成。本 Task 不批准任何阶段 Gate。

### P3-04 — DeepSeek AI Trading Plan Provider

- 结果：实现版本化 `ironpilot-deepseek-trading-prompt-v1`，把 Context 中完整 120 根 15m/1h 原始 K 线、派生指标/形态、一级盘口、instrument rules、账户/余额/受管持仓/活动订单和用户最大亏损授权同时交给 DeepSeek；Prompt 冻结七类 `AITradingPlan v3` 动作及精确 Decimal 输出要求，不注入 Strategy Space、Materializer、anchor、risk tier 或本地交易推荐。DeepSeek V4 Flash/Pro `/chat/completions` 已重构到 `async-openai 0.41.1`：SDK 负责 OpenAI-compatible base URL、认证、chat path、标准请求序列化、HTTP 执行合同和响应解码；BYOT 承载 DeepSeek `thinking` 与 cache usage 扩展；API key 仅从 `IRONPILOT_DEEPSEEK_API_KEY` 或显式构造参数进入 SDK secret config，不进入 YAML、Prompt 或证据。
- 边界与预算：IronPilot 自有代码只保留 Prompt、严格领域解析、usage/cost/latency、预算、一次 replan 和项目必须的 bounded evidence service。该 service 在 SDK 反序列化前流式限制 128 KiB、捕获精确原始响应并重建同一 bounded response；安装自有 service 会替换 SDK 默认 retry executor，确保一次预算 attempt 最多一次 HTTP 请求。空输出、截断、未知字段、浮点/非法方案、provider refusal、HTTP/传输/超时、超大响应、future/expired Context、模型或 usage 不一致全部 fail closed，不生成本地替代方案；call/token/cost 任一预算耗尽均在 HTTP 前拒绝；拒绝反馈仍限定同一 Context 最多一次显式 replan。
- 证据：`ai_provider_attempts` 继续记录 prompt version/hash、完整 request/response、model/vendor ID、finish reason、usage、精确费用、延迟、outcome 和 replan 标记；租约保护、原子持久化、幂等和每 Context 一次 replan 数据库约束不变。13 项专项测试除原有 Prompt/动作/strict parse/预算/持久化覆盖外，新增验证 SDK 429 隐藏重试为 0、单 attempt 仅一个 HTTP request、HTTP 错误原始响应逐字保留、128 KiB 上限在 SDK 解码前拒绝及成功响应精确证据。
- Commit：原始实现 `98f30dddf68ab8fc88c522d9a24e1c2c123da3c7`；开源库重构 `7928509ddad5ff65e8ab4e8540db86b0d6cc00c6`；独立计划修订 `d39229b4f734d77c280bb3a8e614b5f9df8ff358`。
- 门禁：全工作区 113 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 8.30.1 Git 历史与工作树扫描、SDK 单请求/原始证据/响应上限断言和 `git diff --check` 全部通过；开发计划 v3.1.0 已在独立提交升版，重构提交本身未修改计划。
- 已知限制：本 Task 不执行 P3-13 的 `ACCEPT/REJECT`，不生成 P3-05 的 OrderIntent/Paper 订单，也不实现 P3-06 的业务主循环；确定性仓库门禁使用本地 HTTP 协议服务，不消耗真实 DeepSeek API。Task 验收不批准任何阶段 Gate。

### P3-13 — Execution Validation 与 User Authorization

- 结果：实现版本化 `ironpilot-execution-validator-v1`，对已持久化 `AITradingPlan v3` 只做 `ACCEPT/REJECT`：逐项绑定 Context/AI plan/action/TradePlan ID 与 hash，核对 schema、TTL、Spot instrument scope、当前 rules hash、Portfolio/受管持仓/活动订单、用户最大亏损授权、部署模式和 AI 权限；Context 后账户事实、规则或授权变化全部 fail closed。订单兼容性覆盖 exact tick/quantity step、Limit/Market 数量上限、min order amount、Market IOC、精确 Spot `buyLmt`/`sellLmt`、余额、受管资产和冲突订单，不执行本地舍入、缩量、调价、移动止损或替换目标。
- 最大亏损与反馈：`OPEN_LONG` 独立使用 entry/stop/quantity、双边 taker fee 和 AI `max_slippage_quote` 重算最坏亏损；`MODIFY_PROTECTION` 使用当前受管数量、平均入场和新/现有保护价重算；AI 声明值低于重算值或任一值超过当前用户授权均整体拒绝。拒绝码和 bounded 说明可直接构造同一 Context 的一次 `AiPlanRejectionFeedback`；accepted evidence 只绑定原始 canonical plan hash，entry/quantity/stop/target 等任一字段变化都会失去授权。
- 持久化：SQLite migration 新增 `execution_validations`；有效单实例租约下原子核对 AI ledger evidence、持久化 outcome/loss/rejection/hash、更新 action 为 `VALIDATION_ACCEPTED/REJECTED`、将 `OPEN_LONG` TradePlan 从 `PROPOSED` 转为 `ACCEPTED/REJECTED` 并追加审计。action ID 是幂等键，相同重放业务效果为 0，内容冲突 fail closed；无论 ACCEPT 或 REJECT 均不写 `order_intents` 或订单。
- Commit：`86203e92147acfd5671b158fd1edf546685a9347`。
- 门禁：6 项 Execution Validator 专项测试和 2 项 SQLite validation ledger 测试覆盖 tick/qty/min amount/价格限制、费用与滑点最坏亏损、超授权、陈旧 Context、冲突订单、Observe-only、entry/quantity/stop/target 改写检测、拒绝反馈、ACCEPT/REJECT 原子状态、幂等和非法方案订单为 0；全工作区 121 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 46 提交历史与源码工作树扫描、ACCEPT/REJECT only/无订单写入/无旧权限模型静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 不创建 `OrderIntent`、不模拟 Paper 成交、不调用实时价格限制 API、不授权 Testnet/Live，也不实现业务主循环；新鲜公开/账户事实由后续 Runtime/Adapter 组合提供，P3-05 负责消费 accepted evidence 实现 Paper Execution。本 Task 完成不批准任何阶段 Gate。

### P3-05 — 现货 Paper Execution

- 结果：实现版本化 `ironpilot-spot-execution-v1` 与 `SpotExecutionPort`，由同一 provider-neutral 合同承载 Paper、Backtest 和 Testnet venue；`SpotExecutionRequest` 只能从已 `ACCEPT` 且 hash 未改变的 Context、Validation 与 `AITradingPlan v3` 构造，逐字段保留 AI 的 Market/Limit、quantity、limit/trigger price、TIF、expiry、最大滑点、止损和多目标止盈，不执行本地舍入、调价、缩量或参数替换。提交回执只表示订单已持久化，不冒充成交。
- 撮合与状态：实现 `ironpilot-paper-matching-v1`，使用精确 Decimal 模拟 Limit/Market、maker/taker fee、受 AI 最大滑点约束的 Market slippage、可用流动性驱动的部分成交、保护止损和止盈；任何 observation 的行情事实生成时间不晚于 AI Context `as_of` 时整笔拒绝并回滚。OPEN_LONG 入场完全成交后才激活保护单；买入 Fill 创建 ManagedLot，卖出 Fill 仅消费可证明的 ManagedLot；止损或退出清空受管数量后关闭 TradePlan 并撤销剩余保护单。
- 持久化：SQLite migration 新增执行提交、规范订单字段和市场 observation 三张表；有效单实例租约下原子复核 `execution_validations` 的 ACCEPT/hash 证据，写入 execution submission、OrderIntent、PaperOrder、订单规范和审计。action 与 observation 均为幂等边界：相同请求重放业务效果为 0，内容冲突 fail closed；成交、费用、ManagedLot、订单/Action/TradePlan 状态和审计在同一事务提交。
- Commit：`e05918fc57a3b51f9b94d6d42cc1a9b05e1ab7b9`。
- 门禁：3 项 application 撮合/映射测试与 1 项 SQLite 端到端测试覆盖 AI entry/stop/take-profit 字段逐项一致、稳定 request hash、Limit 部分成交、Market 滑点与 maker/taker fee、同一决策事实拒绝、提交/observation 重放零副作用、保护单延迟激活、止损清仓、ManagedLot 累积/消耗及原子状态推进；全工作区 125 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 48 提交历史与源码工作树扫描、共享 port/无同事实成交/幂等/原样字段/ManagedLot 静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 提供共享 port 合同和 SQLite Paper adapter；Minimal Historical Harness、Testnet adapter、业务主循环、私有流同步与 Emergency Core 分别由其后续 Task 实现。本 Task 完成不批准任何阶段 Gate，也不授权 Testnet/Live 写入。

### P3-10A — Minimal Historical Harness

- 结果：实现版本化 `ironpilot-minimal-historical-harness-v1`，以录制的完整 `AiTradePlanLedgerEntry` 和固定 Validation/订单 ID/行情 observation 为输入，复用现有 `ExecutionValidator`、`SpotExecutionRequest::from_accepted_plan` 与 `SqlitePaperExecutionPort`，完整运行 AI Plan → Validation → TradePlan → Paper；费用、滑点、部分成交、保护单和 ManagedLot 继续由既有 Paper policy/matching engine 执行，不包含 AI Provider、HTTP、Prompt、实时 LLM、Materializer 或后置 Risk Engine。
- 确定性与无前视：每个成功运行生成绑定 Context/plan/validation/execution-request hash、稳定 Fill ID 和逐 observation 累计 hash 的确定性账本报告；相同输入在独立 SQLite 数据库中产生相同报告和规范账本行，追加后续 observation 不改变已有记录与累计 hash 前缀。任何 observation 使用不晚于 Context `as_of` 的决策事实、乱序时间、重复 ID、错误标的、空输入或超过 10,000 条资源上限时，均在写入前 fail closed；Paper adapter 保留事务内第二重决策事实复用检查。
- Commit：`5f8b097220e94caed14ba74d1513410819370e14`。
- 门禁：2 项专项测试覆盖独立数据库确定性、规范 SQLite 账本相等、累计 hash 前缀不变、maker fee/精确成交、保护止损扩展和决策事实复用零写入；全工作区 127 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 51 提交历史与源码工作树扫描、共享 Validator/TradePlan/Paper 与无实时 LLM/Materializer/Risk Engine 静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只证明最小可复现执行链，不实现完整绩效统计、优化、参数搜索、组合分析或 P3-10B Full Historical Strategy Evaluation；不授权 Testnet/Live 写入，也不批准 `P3-VS` 或任何阶段 Gate。

### P3-06 — AI 主导现货 Paper Runtime

- 结果：实现版本化 `ironpilot-ai-paper-runtime-v1`，以有界 cycle 运行 Facts → `AiDecisionContext` → DeepSeek/录制 provider → `AITradingPlan v3` → 不可变 AI ledger → `ExecutionValidator` → 原样 `SpotExecutionRequest` → SQLite Paper execution → 后续 AI review/exit。正常交易的 entry、quantity、stop、take-profit、slippage、expiry 和 review 参数全部来自 AI；Runtime 只创建 ID、时间戳、协议 envelope 和审计，不修复、舍入、缩量、调价或替换方案，报告固定记录 `local_parameter_mutations = 0`。
- Provider 与复评：新增有界、哈希化的 Runtime Prompt v2，向真实 DeepSeek provider 交付活动 TradePlan ID、原始 `AITradingPlan` 和最近执行结果，使 `HOLD`、`MODIFY_PROTECTION`、`REDUCE`、`EXIT` 能精确引用管理目标；非 Runtime 的 Prompt v1 保持兼容。Provider 失败、无效方案或预算耗尽均无本地 fallback；Validation 拒绝最多触发同一 Context 的一次完整 replan。
- 追溯与恢复：新增 append-only `paper_runtime_events`，按 cycle 保存无间隙事件序列、完整有界 provider runtime state、Context/plan/validation/execution request hash、Paper observation/fill 和终态报告，并以数据库触发器禁止更新或删除。已完成 cycle 重启返回 `DuplicateNoEffect`，不再次调用 AI 或下单；未完成 cycle 返回 `RecoveryRequired`，阻止在恢复持久化事实前开展新 AI 工作。
- Commit：`f5a7bc061b06b1be471ab57eda740595d7f361d6`。
- 门禁：3 项 Runtime Prompt/真实 provider request 测试与 3 项 SQLite Paper Runtime 测试覆盖活动计划目标与原始方案输入、Prompt v1 兼容、跨标的状态拒绝、多标的隔离、预算耗尽、AI 无效方案、超授权后一次 replan、陈旧数据、Context 后订单变化、持仓复评退出、部分成交、完整/未完成重启和零重复订单；全工作区 133 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny 0.19.4 advisories/bans/licenses/sources、Gitleaks 53 提交历史与源码工作树扫描、AI 参数零本地改写/完整追溯/无新闻 Materializer Risk Engine 静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 运行有界 Paper decision cycle，长期调度、完整历史评估和长时间 Paper 安全分别由后续 Task 承担；确定性门禁使用录制 provider 或本地 HTTP 协议服务，不消耗真实 DeepSeek API；不授权 Testnet/Live 写入，也不批准 `P3-VS` 或任何阶段 Gate。

### P3-07A — Telegram 通知与只读查询

- 结果：按 DEVELOPMENT_PLAN v3.2.0 和 ADR-0007 完成纠正重构。`teloxide-core 0.13.0` 的 `Bot`、`Requester::get_updates`、`Requester::send_message`、Telegram Update/Message 类型和 SDK response decoder 承载 Bot API 协议；删除项目内自建 endpoint、wire DTO、response envelope、通用 HTTP POST 和协议 JSON 解码。IronPilot 代码只保留 chat allowlist、只读命令映射、SQLite 查询、领域文本、批次上限及 cursor/audit 边界。
- 选型证据：2026-07-26 比较 `teloxide-core 0.13.0`、完整 `teloxide 0.17.0` 与 `frankenstein 0.50.2`。`teloxide` 项目 MIT 许可、持续维护、约 4.2k GitHub stars 且 README 列示 2500+ 公开使用仓库；core crate 提供当前 Task 所需 Bot API client 而不引入 Dispatcher/Dialog/macros。`frankenstein` 持续维护但为 WTFPL，不在项目许可证 allowlist；完整 `teloxide` 覆盖面超过只读 adapter。最终锁定最小 `teloxide-core`，仅启用 `rustls`。
- SDK 默认行为审计：未启用 `throttle`、`trace_adaptor`、`cache_me` 或 `erased`，SDK 不执行自动重试或遥测；SDK 对 HTTP 5xx 有固定延迟但不重试，IronPilot 用外层 Tokio timeout 把整个 SDK 调用限制在配置的 2—30 秒预算内；自定义 Reqwest 0.12 client 禁止 redirect。`cargo-deny` 对 SDK 新增默认 features 与必要重复版本逐项精确锁定并记录原因。
- 协议与安全：SDK 发出正数 timeout 的 `getUpdates` long poll，adapter 校验严格递增 update ID 并使用 `highest update_id + 1` caller cursor；`sendMessage` 使用 plain text 与 `protect_content=true`。入站命令和出站通知均受最多 8 个 chat 的 allowlist 约束；非消息、普通文本和非白名单 chat 不回复。Bot token 只从 `IRONPILOT_TELEGRAM_BOT_TOKEN` 或 secret-bearing 构造器进入，未进入 YAML、响应或错误；生产 origin 固定为官方 HTTPS。更新批次 32、查询行数 20、通知批次 32、Telegram 文本 4,096 字符；SDK 不暴露 response-body byte cap，因此先前“项目在 SDK 解码前限制 256 KiB”的证据已撤销，不再作为验收声明。
- 通知与权限边界：通知只从已提交且数据库级 append-only 的 `audit_log` 构造；调用方仅在整批成功后推进 audit sequence。命令枚举不包含 Pause、Resume、Cancel、Emergency、OPEN_LONG 或 EXIT，未知控制命令只返回只读边界说明，重复查询/通知的交易业务效果为 0。
- Commit：原始实现 `290512c6ce0fdcb1119e3f847f654eeed0bbec00`；DEVELOPMENT_PLAN v3.2.0 与 ADR-0007 `90d01665939456e5a68210be16187ddec927d14f`；SDK 重构 `2eccccea75d68647eadce3291d49d005ff393c8d`。
- 门禁：4 项专项测试覆盖配置/chat allowlist、全部只读查询面与拒绝原因、4,096 字符截断、SDK `getUpdates`/`sendMessage` 请求合同、long-poll offset、非白名单隔离、控制命令拒绝、已确认事件双 chat 通知、查询前后业务表零变化、SDK invalid/rejected response fail closed 和 token 不泄漏；全工作区 137 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks Git 历史与源码工作树扫描、SDK 使用/无自建 wire DTO/无自建 HTTP POST/生产 SQL 零写入/无控制命令/完整查询面静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：Telegram `sendMessage` 没有应用幂等键，远端接受后、caller cursor 持久化前崩溃可能重复通知，因此明确采用 at-least-once 而非网络 exactly-once 语义；`teloxide-core 0.13.0` 当前依赖 Reqwest 0.12，而现有 adapter/`async-openai` 使用 Reqwest 0.13，必要重复版本已在 `deny.toml` 精确审计；SDK response decoder 不提供项目级 body byte cap。门禁使用本地 HTTP 协议服务，不使用真实 Bot token 或调用线上 Telegram；Emergency 业务与受保护 Telegram Emergency adapter 分别由 P3-08、P3-07B 实现；不授权 Testnet/Live 写入，也不批准 `P3-VS` 或任何阶段 Gate。

### P3-08 — Emergency Core

- 结果：实现 `ironpilot-emergency-command-v1` 与 `ironpilot-emergency-core-v1`。统一 `AuthorizedEmergencyCommand` 绑定稳定 action ID、授权主体、授权证据 hash、独立确认 nonce hash、半开有效期、5 分钟硬 TTL、canonical payload 与 SHA-256 幂等 hash；原始凭证和 nonce 不落库。同 action ID 异内容 fail closed；新过期命令零业务写入；已持久化命令允许在 TTL 后继续恢复。
- 安全执行：控制器持有有效 runtime lease 后将系统强制保持 `HALTED`，按 `REQUESTED → ENTRY_DISABLED → ORDERS_CANCELLED → EXPOSURE_REDUCING → COMPLETED` 单调持久化；只撤销存在 `paper_order_specs` 归属证据的活动订单，只消费 `managed_lots` 可证明数量，未知/非受管资产卖出为 0。缺失、过期、future、decision-fact 复用或每批超过 3 个行情 observation 时不猜测价格、不盲目替代；完成后不自动恢复 AI 入场。
- 恢复与证据：新增 `emergency_action_steps` 与 `emergency_fills`，两者由 SQLite trigger 禁止 UPDATE/DELETE；每个 action/plan/observation 的紧急成交具备稳定 ID 与唯一约束。部分流动性会进入 `EXPOSURE_REDUCING`，新 controller 实例可从数据库继续；同命令与同 observation 重放的业务成交、减仓和订单效果均为 0。
- 开源复用评估：P3-08 是 IronPilot 内部领域/持久化编排，不新增外部协议。实现直接复用既有成熟开源 Tokio、SQLx、SQLite、SHA-256、UUID、Serde JSON、精确 Decimal，以及项目共享 `PaperMatchingEngine`/`PaperExecutionPolicy`；依赖清单零变化，未自建协议 client、wire DTO、envelope、轮询、分页或重试逻辑。未来真实 venue adapter 仍须遵守 DEVELOPMENT_PLAN v3.2.0 的成熟 SDK 强制复用规则。
- Commit：`a4545a2e527744298565a58bef6c320a9f3ced70`。
- 门禁：4 项专项测试覆盖命令 TTL/双证据/canonical hash、项目归属订单隔离、部分减仓与重启续跑、TTL 后恢复、重复 observation 零业务效果、同 ID 异内容冲突、完成后保持 `HALTED` 及紧急步骤/成交不可篡改；全工作区 141 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 59 个 Git commits 与 `crates/`/`docs/` 源码扫描、共享 matcher 复用/无协议依赖/依赖清单零变化静态断言和 `git diff --check` 全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异，未批准 `P3-VS` 或任何阶段 Gate。

### P3-07B — Telegram Emergency Adapter

- 结果：实现 `ironpilot-telegram-emergency-v1`，在现有 `TelegramReadOnlyAdapter` 的单一 SDK `getUpdates` 流上增加受保护 Emergency 路由。`/emergency_close_all` 生成 UUID v4 随机 nonce；同一 SDK 解码的 chat ID 与 user ID 必须在各自 allowlist 内，并在 10—120 秒内由同一身份发送 `/confirm_emergency_close_all <nonce>`。命令 TTL 独立限制为 10—300 秒。
- 安全语义：pending challenge 固定上限 16；第一笔确认尝试即消费 challenge，错误 nonce、过期、缺失 SDK user、chat/user 不匹配、畸形命令、重放和容量耗尽全部 fail closed。raw nonce 只在受保护 Telegram 消息与进程内短时存在，不持久化或记录；进入统一命令的是 SHA-256 nonce hash。服务重启主动失效所有未确认 challenge，operator 必须重新发起。
- 权限与执行边界：授权证据绑定 adapter version、chat ID、user ID、Telegram update ID、Emergency action ID 和 issue time。成功确认只构造 P3-08 的 `AuthorizedEmergencyCommand`，不复制撤单、减仓或恢复逻辑，不调用 AI，Telegram confirmation 路径对 trading/audit 表直接写入为 0。SDK poll report 将命令与 next offset 交给 caller；caller 必须先把命令交给幂等 Emergency Core，再持久化 offset。真正的执行进度继续由已提交 audit event 通知。
- UI 边界：普通只读 poll 与菜单保持无 Emergency/Pause/Resume/Cancel All；只有 `poll_once_with_emergency` 且 SDK user 位于 operator allowlist 时，`/help` 才显示受保护 `/emergency_close_all`。所有出站挑战、拒绝与确认文本继续通过 SDK `sendMessage`，并设置 `protect_content=true`。
- SDK 复用：继续使用 `teloxide-core 0.13.0` 的 `Bot`、`Requester::get_updates`、`Requester::send_message`、`Update`、`Message`、`User` 和 SDK response decoder；复用现有 Reqwest 0.12 client timeout/no-redirect 配置。依赖清单零变化，生产代码没有自建 Telegram endpoint、wire DTO、response envelope、HTTP POST、轮询重试或协议错误解析。
- Commit：`1c68737b78669e9c4cd42c129f465e1eceaf025b`。
- 门禁：6 项 Telegram 专项测试（其中 2 项为 P3-07B 新增）覆盖 operator policy/命令解析、chat+user 绑定、随机 nonce、二次确认、错误 nonce 一次性失效、重放零命令、SDK user 缺失/非白名单隔离、受保护 help、SDK `getUpdates`/`sendMessage`/`protect_content` 合同、确认路径业务表零写入及 token 不泄漏；全工作区 143 项测试通过、0 失败，1 项既有 Bybit 线上 WSS 只读测试保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 62 个 Git commits 与 `crates/`/`docs/` 源码扫描、SDK 使用/无自建 wire 协议/依赖清单零变化/开发计划零差异静态断言和 `git diff --check` 全部通过；未批准 `P3-VS` 或任何阶段 Gate。

### P3-VS — Prototype Vertical Slice Gate Evidence Review

- 状态：仓库内技术证据复核完成；用户于 2026-07-26 明确接受现有证据、批准 `P3-VS` 并决定先不执行线上 smoke。Gate 已标记 `DONE`，静态依赖满足的 `P3-10B`、`P3-11` 与 `P4-01` 已解锁。
- 原始 15m/1h、Features、账户与 DeepSeek 请求合同：`deepseek::tests::prompt_contains_raw_market_features_account_rules_and_authorization` 使用两组完整 `FEATURE_CANDLE_WINDOW` 闭合 K 线、实时盘口、`MarketDataSource::WebSocketLive`、实际 `MarketFeatureEngine`、instrument rules、Portfolio 和最大亏损授权构建生产 Prompt；`deepseek::tests::exact_open_long_is_parsed_with_raw_usage_cost_and_latency_evidence` 证明该 Prompt 经 `async-openai` 的真实请求/响应合同产生完整 `OPEN_LONG`、原始响应、usage、费用和延迟证据。
- 在线证据边界：确定性仓库门禁按既有 P3-04 合同使用有界本地 HTTP 服务，不调用线上 DeepSeek；当前环境也没有 `IRONPILOT_DEEPSEEK_API_KEY`。因此本次证明的是“完整、带 `WebSocketLive` 来源标记的确定性事实进入生产 DeepSeek SDK 请求合同”，不是实际行情或一次线上模型调用证明。若 Gate 对“真实 15m/1h 进入 DeepSeek”的解释要求实际线上事实与请求，则该项仍需在秘密注入和外网可用的受控环境另行执行，不能由本次结果替代。
- AI 动作：`deepseek::tests::exact_open_long_is_parsed_with_raw_usage_cost_and_latency_evidence` 覆盖合法 `OPEN_LONG`；`deepseek::tests::multiple_management_actions_parse_without_local_parameter_generation` 覆盖 `NO_TRADE`、`HOLD` 和 `MODIFY_PROTECTION`；`deepseek::tests::runtime_provider_sends_prompt_v2_with_the_management_target` 证明复评 Prompt 绑定目标活动计划。
- 精确方案与 Validator：`exact_plan_is_accepted_without_returning_or_rewriting_trade_fields`、`execution_request_preserves_every_ai_order_and_protection_field` 和 `paper_execution_is_exact_partial_and_idempotent_without_decision_bar_reuse` 逐项证明 entry type/price、quantity、stop、take-profit、TIF、滑点和来源 Plan 原样进入共享 Paper port；Validator 结果面只有 `ACCEPT/REJECT`。
- 非法订单为 0：`tick_quantity_minimum_amount_and_price_limit_fail_closed`、`fees_slippage_and_user_maximum_loss_are_independently_enforced`、`stale_context_and_any_post_validation_field_change_cannot_authorize_execution`、`observe_only_permission_rejects_an_order_bearing_plan`、`a_conflicting_exchange_order_rejects_the_whole_plan` 和 `persistence::tests::ai_paper_runtime_failures_are_traced_and_create_zero_orders` 覆盖非法 tick/qty/minimum、超最大亏损或授权、陈旧数据、Context 后状态变化、冲突订单和无效 AI 输出，订单写入均为 0；拒绝后最多一次有界 replan。
- 订单/成交/持仓复评：`persistence::tests::ai_paper_runtime_opens_reviews_exits_and_restarts_without_duplicate_ai` 完整执行 `OPEN_LONG`、两次部分成交、下一 Context 的持仓复评和 `EXIT`，最终受管仓位为 0、计划为 `CLOSED`、本地参数改写为 0；Provider 合同同时接受 `HOLD` 与 `MODIFY_PROTECTION`。
- restart、审计、对账、Telegram、Emergency：上述 Paper Runtime 测试证明完成周期重启返回 `DuplicateNoEffect` 且不再次调用 AI 或自动开仓，未完成周期 `RecoveryRequired`；`reconciliation_snapshot_and_audit_are_atomic_and_idempotent`、`second_instance_cannot_acquire_lease_or_write_tradable_state`、`backup_is_integrity_checked_and_recoverable`、Telegram 6 项 SDK 专项测试及 `emergency_is_owned_only_idempotent_and_restart_recoverable` 分别证明原子审计/对账、租约隔离、恢复、只读通知/查询、身份二次确认和独立 Emergency 恢复路径。
- 2C2G 与活动链：`every_2c2g_resource_ceiling_is_enforced`、5 项运行时监督测试证明配置/队列/任务/RSS 软门槛受限；`active_domain_surface_contains_no_v2_strategy_or_risk_authority`、v3 crate 导出面和 Prompt 断言证明活动链没有 Materializer、策略型 Risk Engine 或新闻节点，遗留 v2 Risk Engine 仅位于未编译的 `crates/ironpilot-domain/legacy/v2`。
- 成熟 SDK 复用：DeepSeek 使用 `async-openai 0.41.1`，Telegram 使用 `teloxide-core 0.13.0`；P3-VS 复核补齐 `docs/DEPENDENCIES.md` 中遗漏的 Telegram SDK、专用 transport 和 UUID v4 nonce 登记。生产代码没有恢复自建 Telegram endpoint、wire DTO、response envelope、轮询重试或协议错误解析。
- 本次门禁：全工作区 143 项测试通过、0 失败，1 项依赖外网的 Bybit 公共 WSS 只读 smoke 保持显式忽略；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --all-targets --locked`、`cargo metadata --locked --no-deps`、cargo-deny advisories/bans/licenses/sources、Gitleaks 63 个 Git commits 与 `crates/`/`docs/` 源码扫描全部通过；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 权限边界：本证据不授权 Testnet 写、实盘、永续或新闻能力；没有运行任何交易所写请求。

### P3-10B — Full Historical Strategy Evaluation

- Result: implemented the versioned `ironpilot-full-historical-evaluation-v1` offline evaluator for the exact Rule-only Baseline, Deterministic AI Plan Stub, and recorded AI Trading Plan arms. Every comparison binds the same market facts, user maximum loss, decision/settlement cutoffs, execution model, fees, and slippage.
- Immutable evidence: the manifest binds dataset, Context schema, Prompt, Model, deterministic stub plan set, recorded AI plan set, Validator, Execution, matcher, metric library, time split, costs, and stress scenarios. Records additionally bind market-fact, plan, and execution-evidence hashes.
- Mature-library reuse: standard total return, maximum drawdown, and expectancy calculations delegate to `quant-metrics 0.7.0` with exact Decimal adapters. Candidate review and the deliberately narrow dependency choice are recorded in `docs/DEPENDENCIES.md`; project code retains only comparison/evidence orchestration and exact quote-amount reporting.
- Report: full-sample and out-of-sample return, drawdown, expectancy, trade count, costs, rejection reasons, AI contribution, stress results, per-trade differences, and independent reference tie-out are deterministic and canonically hashed.
- Fail closed: future facts, missing or incomparable arms, local AI-plan mutation, missing provenance, safety failures, or independent-reference mismatch reject the evaluation. Profitable outcomes cannot offset a safety failure.
- Boundary: this is offline evaluation only. Rule-only output cannot enter production, and no exchange/model call, Materializer, strategy-style Risk Engine, news node, or composition-root production wiring was added.
- Implementation commit: `d4f732624574758a8972674ac7e19b9a5eef4860`.
- Gates: 3 targeted historical-evaluation tests passed; the full workspace passed 146 tests with 0 failures and 1 explicitly ignored live Bybit public-WSS smoke. Formatting, strict Clippy, locked build/test/metadata, cargo-deny advisories/bans/licenses/sources, Gitleaks history plus `crates/` and `docs/`, boundary checks, and `git diff --check` passed. `docs/DEVELOPMENT_PLAN.md` remained unchanged.
- Gate authority: this completion does not approve any new stage Gate.

## Next Action

P3-11 Long-running Paper Safety is the next recommended task. P4-01 remains
independently READY. Neither task has been started by this completion update.

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
