# IronPilot MVP 交付范围与纵向闭环优先修订

> 历史 v2 文档。与 DEVELOPMENT_PLAN v3 或 ADR-0006 冲突的 Materializer、Risk 依赖和有界 AI 权限内容不再有效。

> 文档状态：`PROPOSED_CHANGE_SPEC`
>
> 日期：2026-07-24
>
> 适用范围：`docs/DEVELOPMENT_PLAN.md`、P2/P3/P4 任务依赖、News Risk Guard、Telegram 控制面、Paper/Backtest/Testnet Gate
>
> 目标：在不削弱资金安全、状态一致性和不可绕过风控的前提下，优先尽快跑通一个可工作的 AI 现货 Paper 纵向闭环，避免把 MVP 扩张成半个通用量化平台。

---

## 1. 修订背景

现有开发计划的安全工程设计完整，但任务依赖和验收门槛存在明显的“全覆盖后才算 MVP”倾向：

- 完整历史策略回测与独立参考先于第一个 AI Paper 闭环。
- 结构化 News Risk Guard 是 DeepSeek 决策链的前置硬依赖。
- 30 天 Paper、72 小时 Testnet、大量故障演练和跨引擎差异解释被集中在较晚阶段，导致项目长期看不到可运行核心产品。
- Telegram 同时暴露 Pause、Resume、Cancel All，可能重新引入用户盘中情绪化干预。
- 开源框架能力矩阵、完整数据库模型、长期留存、独立 parity 和大量文档治理可能优先于产品闭环本身。

这些设计适合作为进入真实资金前的 Release Gate，但不应全部阻塞第一个可运行原型。

IronPilot MVP 的首要产品验证应该是：

> 市场数据能否触发 AI 形成受限策略意图，经过确定性物化、硬编码风控、TradePlan 和 Paper Execution 后，自动完成开仓、持仓管理、退出、审计、通知和紧急关闭。

---

## 2. 核心交付原则

### 2.1 纵向闭环优先于横向完备

优先完成一条真实贯通的最小业务链：

```text
1–3 个配置化现货标的
→ Bybit 公共行情
→ Market Features
→ Eligibility Event
→ DeepSeek Strategy Intent
→ Deterministic Materialization
→ Risk Engine
→ TradePlan
→ Paper Fill
→ 持仓复评与退出
→ Telegram 通知
→ Emergency Close
```

在这条链路没有跑通以前，不应优先建设：

- 通用回测平台。
- 多回测引擎适配。
- 完整新闻事件平台。
- 多 LLM Provider。
- Web UI。
- 多交易所。
- PostgreSQL、高可用或微服务。
- 面向未来所有策略家族的完整抽象。

### 2.2 安全底线不可后置，完备性可以后置

第一个纵向闭环仍必须保留：

- Decimal 金额与交易精度。
- 不可信状态禁止新开仓。
- AI 无执行和风险覆盖权限。
- Risk Engine 不可绕过。
- 订单/成交业务幂等。
- TradePlan 状态机。
- Paper 账户与受管资产语义。
- 审计链。
- 紧急退出。

可以后置的是：

- 完整跨引擎回测验证。
- 30 天稳定性证据。
- 完整新闻 Provider。
- 大规模故障矩阵。
- 长期数据归档优化。
- 独立参考引擎逐笔差异解释。

### 2.3 MVP 不是所有需求点都完成

MVP 的定义是：

> 用最小范围证明核心产品假设和安全边界可以共同成立。

MVP 不应被解释为：

> 所有已知需求、研究能力、平台能力、运维能力和未来扩展点都已经实现。

---

## 3. 新增早期里程碑：Prototype Vertical Slice Gate

在 30 天 Paper Gate 之前新增独立里程碑：

## `P3-VS — AI Spot Paper Vertical Slice`

### 3.1 目标

证明 IronPilot 的核心产品链路已经真实可运行，而不是只有模块、测试和文档。

### 3.2 最小范围

- 支持 1–3 个配置化 Spot 标的。
- 公共 REST/WS 获取真实 Bybit 行情。
- 使用已闭合 K 线生成版本化 Market Features。
- Eligibility/Event Prefilter 触发 AI 决策。
- DeepSeek 生成严格 Strategy Intent。
- 确定性物化为入场、止损、目标、数量和订单参数。
- Risk Engine 审批或拒绝。
- TradePlan 持久化完整生命周期。
- Paper Execution 模拟 Limit/Market、费用和基础滑点。
- 至少完成一次正常开仓到正常退出的闭环。
- 至少完成一次 AI `NO_TRADE` 或 Risk `REJECTED` 链路。
- 支持服务重启后恢复活动 TradePlan。
- Telegram 汇报决策、开仓、平仓、异常和紧急操作。
- Emergency Close 能安全关闭全部受管 Paper 敞口。

### 3.3 Gate 硬指标

| 指标 | 目标 |
|---|---:|
| 未经过 Risk Engine 的 Paper 订单 | 0 |
| AI 非法输出产生订单 | 0 |
| 重复业务订单效果 | 0 |
| 未审计 TradePlan 动作 | 0 |
| 重启后重复入场 | 0 |
| Emergency Close 重复卖出 | 0 |
| 正常完整 TradePlan 生命周期 | 至少 1 条可审计证据 |
| 拒绝/无交易生命周期 | 至少 1 条可审计证据 |

### 3.4 明确不属于该 Gate 的条件

以下不得阻塞 `P3-VS`：

- 30 天 Paper soak。
- 72 小时 Testnet。
- Freqtrade 独立参考。
- NautilusTrader/Barter 完整 capability matrix。
- 全量历史新闻数据。
- 结构化新闻 Provider 付费接入。
- 100 次 WS 断连演练。
- 50 种恢复状态组合。
- 完整策略盈利证明。
- 多年数据保留和 PostgreSQL 迁移评估。

`P3-VS` 完成只表示核心产品链路存在，不授权 Testnet 或真实资金。

---

## 4. P3-10 不再阻塞第一个 Paper 纵向闭环

## 4.1 当前问题

完整 P3-10 同时承担：

- 历史策略回测。
- 回测框架调研。
- NautilusTrader/Barter 能力评估。
- Freqtrade 独立参考。
- 样本外和压力测试。
- 跨引擎逐笔差异解释。

若 P3-06 必须等待全部 P3-10 完成，项目会优先建设研究平台，而不是验证 AI 自动交易闭环。

## 4.2 修改方案：拆成两个层次

### `P3-10A — Minimal Historical Harness`

在 `P3-VS` 前只要求：

- 可控历史时钟。
- 固定历史 K 线输入。
- 相同 Market Feature、Strategy Intent Stub、Materializer、Risk、TradePlan 和 Paper Execution 语义。
- 无收盘价生成并在同一收盘价成交的明显前视错误。
- 费用和基础滑点模型。
- 相同输入可复现相同交易账本。

它是领域正确性测试工具，不是通用回测平台。

### `P3-10B — Full Historical Strategy Evaluation`

在 `P3-VS` 之后，与实时 Paper 并行推进：

- Rule-only / Decision Stub / AI 三组对照。
- train/validation/forward 或 walk-forward。
- 完整绩效、成本和压力报告。
- 开源回测组件评估。
- 必要时使用 Freqtrade 离线独立参考。
- 跨引擎差异解释。

`P3-10B` 必须在长期 Paper 版本升级为 `entry_enabled`、进入 Testnet 或真实资金之前通过，但不阻塞第一个可运行 Paper 纵向闭环。

## 4.3 修改后的依赖

```text
P3-05 Paper Execution
P3-04 AI Strategy Provider
P3-09 Strategy Materializer
P3-02 Risk Engine
        ↓
P3-10A Minimal Historical Harness
        ↓
P3-VS AI Spot Paper Vertical Slice
        ↓
 ┌───────────────┬──────────────────┬─────────────────┐
 │ P3-10B 回测   │ 长期实时 Paper    │ News Guard 增强 │
 └───────────────┴──────────────────┴─────────────────┘
        ↓
长期 Paper Gate / Testnet Gate
```

---

## 5. News Risk Guard 支持可降级 MVP 模式

## 5.1 原则

News Risk Guard 是安全增强能力，不是第一条 AI Paper 纵向链路成立的必要条件。

MVP 不应因为缺少昂贵、复杂或不稳定的结构化新闻源而无法运行 Paper。

## 5.2 三种运行模式

```yaml
news_guard:
  mode: disabled | scheduled_blackout | structured_provider
```

### `disabled`

适用范围：

- 本地开发。
- Replay。
- 第一个 Paper Vertical Slice。

行为：

- 明确记录 `NEWS_PROTECTION_UNAVAILABLE`。
- 不宣称具备新闻风险保护。
- 不允许用于真实资金。
- 其他数据、账户和风险门禁保持不变。

### `scheduled_blackout`

适用范围：

- MVP Paper。
- 没有可靠结构化 Provider 时的保守模式。

能力：

- 手工或配置化维护宏观事件时间窗。
- 交易所维护时间窗。
- 已知代币解锁、治理或系统升级窗口。
- 黑名单日期或时段。

行为：

- 进入窗口后禁止新开仓或转 `OBSERVE_ONLY`。
- 不解析自由文本新闻。
- 不宣称能识别突发黑天鹅。

### `structured_provider`

适用范围：

- 完整 Paper Gate。
- Testnet 前增强。
- 真实资金前按最终 Release Gate 决定是否强制。

保留现有：

- Provider、事件 ID、发布时间、影响范围、严重度和 TTL。
- veto-only。
- 过期或完整性失败时降低权限。

## 5.3 任务依赖修改

- `P3-04 DeepSeek Decision Provider` 不再硬依赖完整 `P2-05 Structured News Risk Guard`。
- `P3-VS` 可使用 `disabled` 或 `scheduled_blackout`。
- `P2-05` 改为与纵向闭环并行的增强 Task。
- 进入长期 Paper/Testnet 前必须明确当前 News Guard 模式和剩余风险。
- 真实资金不得默认使用 `disabled`。

---

## 6. Telegram 控制面与情绪化干预隔离

## 6.1 产品目标

Telegram 的主要职责是：

- 汇报系统正在做什么。
- 展示当前可信状态。
- 在严重异常时提供紧急退出。

它不应成为用户盘中频繁干预 AI 交易计划的遥控器。

## 6.2 默认用户菜单

建议只展示：

- `System Status`
- `Current Positions`
- `Active TradePlans`
- `Recent Trades`
- `Risk Status`
- `Emergency Close All`

`Emergency Close All` 保留二次确认、幂等和完整审计。

## 6.3 Pause / Resume / Cancel All 的处理

这些能力可以保留用于运维，但默认不放在日常用户菜单中。

### `Pause New Entries`

- 仅管理员维护入口可见。
- 必须填写受控 reason code。
- 记录操作者、原因和持续时间。
- 不允许改变已有保护和退出逻辑。

### `Resume`

- 不直接恢复交易。
- 只发起全量同步、对账和风险检查。
- 检查通过后最多进入 `READY`，再由运行策略决定是否启用交易。

### `Cancel All Orders`

- 仅用于运维或紧急状态。
- 只撤 IronPilot 可证明归属的订单。
- 若撤单会让已有仓位失去保护，必须拒绝或先进入受控退出流程。

### 推荐入口

- 本机受保护 CLI。
- loopback-only 管理 API。
- Telegram 隐藏管理员菜单，而非普通主菜单。

核心原则：

> 日常用户可以观察系统，也可以紧急退出，但不能因为短时恐慌随意改写正常交易流程。

---

## 7. 30 天 Paper Gate 后置

30 天 Paper 仍然保留，但其含义必须修正。

### `P3-VS` 回答

> 核心产品能否工作？

### 30 天 Paper Gate 回答

> 核心产品能否长期稳定、安全和可重复地工作？

因此：

- 第一次可运行原型不需要等待 30 天。
- `P3-VS` 后立即启动长期 Paper soak。
- 回测、News Guard、故障注入、资源画像和策略改进可以在 soak 期间并行推进。
- 30 天证据是进入 Testnet/更高权限阶段的 Gate，不是开发团队第一次看到系统自动交易的 Gate。

---

## 8. 控制工程重量的强制规则

## 8.1 Vertical Slice 前禁止事项

在 `P3-VS` 完成前，除非直接阻塞核心链路，不实施：

- 通用插件系统。
- 多 LLM Provider。
- 多交易所抽象的第二个实现。
- Web UI 或移动端。
- 微服务、Kafka、Redis、Kubernetes。
- PostgreSQL。
- 自动 Hyperopt。
- 通用策略 DSL。
- 通用回测引擎。
- 完整新闻聚合平台。
- 自动模型选择或 Agent 工具调用。

## 8.2 开源依赖评估适度化

引入依赖仍要检查许可证、维护状态和安全，但 Vertical Slice 前只回答：

1. 是否满足当前明确需求？
2. 是否能在 2C2G 下运行？
3. 是否存在明显安全或维护风险？
4. 如果不采用，最小替代实现是什么？

不得为了“选出理论最优框架”同时做多个大型框架的完整 PoC。

## 8.3 文档预算

每个 Vertical Slice 前 Task 只要求：

- 目标。
- 范围。
- 关键不变量。
- 最小接口。
- 验收测试。
- 已知限制。

不要求提前穷举未来所有扩展、故障和数据库字段。

## 8.4 数据库范围

Vertical Slice 前只实现核心表：

- system state
- market snapshots / events
- strategy intents
- materialized trade parameters
- risk decisions
- trade plans
- order intents / paper orders / fills
- managed lots
- emergency actions
- audit log

长期归档、独立回测表、完整 news 表和高级性能表可在对应能力开始时再增加。

## 8.5 “先证明需要，再抽象”

- 一个 Adapter 实现不证明需要复杂工厂或插件注册表。
- 一个策略空间版本不需要通用 DSL。
- 一个交易所不需要提前实现跨所路由。
- 一个 LLM Provider 只需要窄 `DecisionProvider` 边界。
- 一个 Paper 撮合模型不需要自研通用交易框架。

---

## 9. 推荐修改后的开发路线

## Phase A — Minimal Safety Kernel

- Rust 工程骨架。
- 核心领域类型。
- 最小配置。
- SQLite 核心存储。
- 系统状态机、Risk、TradePlan 和审计。

## Phase B — Market to AI

- Bybit 公共 REST/WS。
- Market Features。
- Eligibility/Event Prefilter。
- DeepSeek Strategy Intent。
- News Guard 使用 `disabled` 或 `scheduled_blackout`。

## Phase C — AI to Paper Execution

- Strategy Materializer。
- Paper Execution。
- 持仓复评和退出。
- Telegram 通知与 Emergency Close。
- Minimal Historical Harness。

## Phase D — Prototype Vertical Slice Gate

完成 `P3-VS`，形成第一个可运行 IronPilot。

## Phase E — Parallel Hardening

并行推进：

- 30 天实时 Paper。
- Full Historical Strategy Evaluation。
- AI 增量价值对照。
- Structured News Provider。
- 故障注入。
- 资源和数据库增长优化。
- Telegram 运维入口收敛。

## Phase F — Testnet and Release Gate

长期 Paper、完整回测、安全证据和必要增强通过后，进入 Bybit Testnet。

---

## 10. 对 DEVELOPMENT_PLAN 的具体修改

### 10.1 新增 Task

新增：

- `P3-10A Minimal Historical Harness`
- `P3-VS AI Spot Paper Vertical Slice Gate`
- `P3-10B Full Historical Strategy Evaluation`

### 10.2 修改依赖

- `P3-04` 删除对完整 `P2-05` 的硬依赖。
- `P3-06` 不再依赖完整 `P3-10B`。
- `P3-VS` 依赖 `P3-04`、`P3-05`、`P3-09`、`P3-10A`、核心 Telegram/Emergency 能力。
- `P3-10B`、结构化 News Guard 和长期 Paper 在 `P3-VS` 后并行。
- Testnet Gate 继续依赖完整回测和长期 Paper 证据。

### 10.3 修改 P2-05

从“AI Paper 的前置硬门禁”改成“安全增强能力”，支持三种模式。

### 10.4 修改 P3-07 / P3-08

- 普通 Telegram 菜单去除 Pause、Resume、Cancel All。
- 保留状态查询和 Emergency Close。
- 运维控制移到受保护管理入口。

### 10.5 修改第 28 节 Gate

把 Gate 拆成：

1. `Prototype Vertical Slice Gate`
2. `Long-running Paper Safety Gate`
3. `Historical Strategy Evidence Gate`
4. `Bybit Testnet Gate`
5. `Live Release Gate`

不得再把全部标准压缩成“完成第一个可运行系统”的单一门槛。

---

## 11. 最终验收原则

完成本修订后，项目应遵循：

- 先让 IronPilot 真正完成一笔受限自治 Paper 交易，再继续完善平台能力。
- 回测与实时 Paper 可以并行演进，但真实资金前必须完成严格证据 Gate。
- News Guard 是可升级安全层，不能成为第一条 Paper 链路的单点阻塞。
- Telegram 默认鼓励观察和紧急退出，不鼓励盘中情绪化暂停、恢复和撤单。
- 30 天 Paper 证明长期可靠性，不再定义第一次原型何时出现。
- 每个抽象、框架和文档必须服务于当前阶段的明确问题，不能为了覆盖未来需求而提前建设。

最终原则：

> **先跑通一个安全、真实、可审计的 AI Paper 闭环，再逐步把它做严谨；不要先造完半个平台，最后才验证产品是否成立。**
