---
status: accepted
date: 2026-07-25
supersedes:
  - 0002-deterministic-trade-parameters-and-news-guard
amends:
  - 0003-versioned-market-features-and-pattern-semantics
  - 0004-compositional-historical-backtesting
---

# 采用有界 AI 策略权限

IronPilot 采用有界 AI 策略权限：

> AI has bounded strategy authority, but no execution authority and no authority to override deterministic risk constraints.

AI 在预授权、版本化且可验证的 Strategy Space 内选择交易策略。确定性系统负责校验和物化该选择、裁决风险、持久化 TradePlan、执行、恢复与审计。AI 不能访问账户密钥、Exchange Adapter、工具、配置、文件系统或 Shell，不能覆盖风险上限，也不能直接产生交易副作用。

三种权限必须分离：

| 权限 | 权威组件 | AI 权限 |
|---|---|---:|
| 策略权限 | AI Strategy Decision Provider | 在版本化白名单内拥有 |
| 风险权限 | Deterministic Risk Engine | 无 |
| 执行权限 | TradePlan + Execution + Exchange Adapter | 无 |

## Decision

当前权威业务链为：

```text
已闭合 K 线与版本化 Market Features / Pattern Observations
→ Eligibility / Event Prefilter
→ AI Strategy Intent v2
→ Schema / Serde / Semantic Validation
→ Deterministic Strategy Materialization
→ Deterministic Risk Engine
→ 持久化 TradePlan
→ Execution Preflight
→ Paper Execution 或另行授权的 Bybit API Execution
→ Reconciliation / Position Review
```

当前默认链没有新闻步骤，也没有 `disabled` 新闻占位节点。Replay、Paper 和 Backtest manifest 不要求新闻数据，系统不得声称具备新闻风险保护。未来引入新闻能力前，必须先修订开发计划和相关 ADR。

### Strategy Intent

`StrategyIntent v2` 是 AI 的结构化策略合同。`P3-VS` 前只有 `strategy-space-v1-vs` 可执行；未知版本、枚举、字段或非法组合必须 fail closed。AI 可以在该版本白名单内选择 action、strategy family、entry/stop/target policy、受控 anchor、risk tier、持有期限、复评和失效条件。

AI 不得输出或控制自由绝对价格、任意数量、账户资金百分比、杠杆、账户、订单 ID、幂等键、重试、风险上限或运行权限。`Candidate Decision` 仅作为历史术语保留，不是当前运行时合同。

### Eligibility / Event Prefilter

Prefilter 只判断数据、事件、状态、去重、冷却和 LLM 预算是否允许调用 AI，并可以生成受控的结构变化或复评事件。它不得断言“有效做多机会”或“有效卖点”，不得用指标或形态决定最终方向，也不得固定选择入场、止损、目标或退出策略。`Rule Prefilter` 仅作为历史术语保留。

### Deterministic Strategy Materialization

Materializer 把已验证 Strategy Intent 中由 AI 选择的 anchor、policy 和 risk tier，结合不可变 Market Feature Snapshot、Portfolio Snapshot、Instrument Constraints、Risk Config 与版本化算法转换为精确候选价格、数量和订单参数。

Materializer 可以因锚点缺失、约束冲突、最低盈亏比、精度或风险上限而拒绝或收紧，但不能替换 strategy family、entry、stop、target 或方向，也不能在物化失败时另选策略继续交易。代码模块可暂时保留 `trade_parameters` 名称；其领域职责仍是物化器，不是第二策略引擎。

### Risk and execution

Risk Engine 只能批准、向下调整、拒绝或降低系统权限，不能扩大 AI 申请的风险，也不能替换策略。任何可执行动作必须先形成持久化 TradePlan 和 OrderIntent，再通过 Execution Preflight。REST ack 不等于成交；未知结果必须冻结同目的新动作并对账。

## Superseded scope

本 ADR 取代 ADR-0002 中以下决策：

- 本地规则先生成交易候选或决定方向；
- 新闻守卫是调用 LLM、Replay、Paper 或 Backtest 的默认前置步骤；
- LLM 只对本地规则已经设计的交易作批准或拒绝；
- Trade Parameters Calculator 可以独立设计入场、止损、目标或退出方案。

ADR-0002 中“模型无执行权限、精确参数由确定性代码产生、Risk Engine 不可绕过、MCP 不进入运行时交易边界”等安全目标由本 ADR 重新确认。

ADR-0003 的特征迁移和版本化决定保持有效；只读特征的下游职责改用本 ADR 的 Eligibility / Event Prefilter、Strategy Intent 和 Materialization 语言。ADR-0004 的组合式回测与独立参考决定保持有效；历史链改为复用当前权威业务链，并按 `P3-10A` 与 `P3-10B` 分层。

## Considered Options

- 本地规则设计完整交易，AI 只批准或拒绝：实现简单，但 AI 没有实际策略权限，无法验证产品核心假设。
- AI 直接控制价格、数量、风险和执行：策略自由度高，但不可复现且能绕过账户安全边界。
- AI 在版本化白名单内选择策略，确定性系统物化、裁决和执行：保留真实策略选择权，同时维持不可绕过的风险和执行边界；本项目采用此方案。

## Consequences

- Strategy Space、Prompt、Schema、Materialization 和 Risk Rules 必须独立版本化并可审计。
- 非法或无法物化的 Intent 结果是 `NO_TRADE` 或 `REJECTED`，不得由本地代码改选策略。
- Market Features 和 Pattern Observations 是只读事实，不拥有交易方向或策略选择权。
- Paper、Replay 和 Backtest 必须复用同一策略权限、物化、风险与执行语义。
- AI 表现弱于 Rule-only 基线可以阻止策略版本升级，但不能改写工程正确性或安全结论。
- 任何新闻能力、额外策略家族或执行权限扩展都必须通过独立版本与计划变更进入，不得静默加入当前合同。
