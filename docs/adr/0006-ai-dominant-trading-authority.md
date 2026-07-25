---
status: accepted
date: 2026-07-25
supersedes:
  - 0005-bounded-ai-strategy-authority
amends:
  - 0003-versioned-market-features-and-pattern-semantics
  - 0004-compositional-historical-backtesting
---

# 采用 AI 主导的完整交易决策权限

## Context

IronPilot 的产品初衷是最大化 AI 对行情、账户和交易状态的综合判断能力，由 AI 直接制定交易方案。ADR-0005 采用的有界 Strategy Space、确定性 Strategy Materializer 和后置 Risk Engine，把 AI 限制为预设策略模板选择器，精确入场、数量、止损和止盈实际由本地算法形成。这使本地传统策略框架重新成为交易权威，与产品初衷冲突。

用户在 2026-07-25 明确确认：

> 系统监听 Bybit 15m/1h K 线，计算 RSI、EMA 等指标和 K 线形态，连同当前账户资金情况和账户可接受的最大亏损金额通过 Prompt 传给 AI；AI 制定完整交易方案；执行接口按照 AI 方案下单。不得用 Materializer 或后置策略型 Risk Engine 限制 AI。

## Decision

IronPilot 采用 AI 主导的完整交易决策权限：

> AI has full trading-decision authority within the user-authorized account and product scope. Deterministic components may validate or reject, but must not design, materialize, optimize, resize, or rewrite the AI trading plan.

### 权限划分

| 权限 | 权威组件 |
|---|---|
| 市场、账户、订单和成交事实 | Market/Portfolio/Reconciliation |
| 完整交易方案 | AI Trading Decision Provider |
| 用户资金和部署授权 | User Configuration |
| Schema、协议和授权合法性 | Execution Validator |
| 忠实执行、幂等和恢复 | TradePlan + Execution |
| 外部订单和余额事实 | Bybit |

AI 直接决定：

- 是否交易和 Spot 合法方向；
- order type、精确 entry、quantity、stop、take-profit；
- 最大滑点、有效期、最大持有时间和复评时间；
- HOLD、CANCEL_ENTRY、MODIFY_PROTECTION、REDUCE、EXIT；
- thesis、confidence、失效条件和风险说明。

本地系统不得：

- 使用 Strategy Space 白名单限制 AI 只能选择预设模板；
- 从 anchor、policy 或 risk tier 推导精确参数；
- 用 Materializer 形成 entry、quantity、stop 或 target；
- 用后置 Risk Engine 审批、收紧、优化或改写 AI 方案；
- 在 AI 方案无效时本地另造或修复一笔交易；
- 使用技术指标组合预先认定“有效交易机会”。

### 权威业务链

```text
Bybit 15m/1h Kline + Price + Book
→ Raw OHLCV + Market Features + Pattern Observations
→ Balance + Managed Assets + Positions + Orders
→ User Maximum Loss Authorization
→ Versioned AI Decision Context
→ DeepSeek AITradingPlan v3
→ Execution Validation / User Authorization Check
→ Persisted AI TradePlan and OrderIntent
→ Paper or Authorized Bybit Execution
→ Order / Fill / Balance Reconciliation
→ Next AI Review Context
```

活动链中没有 Strategy Materializer 或策略型 Risk Engine。

### Validation 不是策略裁决

Execution Validator 只允许 `ACCEPT` 或 `REJECT`，负责：

- strict Schema、Decimal、单位、TTL；
- Spot-only、order type 和 time-in-force；
- tickSize、qtyStep、minNotional、价格限制；
- 余额、受管资产、活动订单和状态；
- AI 声明最大亏损与本地独立重算；
- 用户最大亏损和部署权限；
- 幂等和状态迁移。

Validator 不得调价、缩量、移动止损、替换目标或添加本地保护策略。非法方案必须整体拒绝并把原因反馈 AI；本地不得修复后继续执行。

用户最大亏损金额是硬授权，而不是本地策略 Risk Engine。它限制账户授权范围，但不判断交易思想。超限只拒绝，不自动修改 AI 方案。

### Context 不能只提供传统指标摘要

AI 必须同时获得：

- 有界的原始 15m/1h 已闭合 OHLCV 序列；
- 当前价格与盘口；
- RSI、EMA、ATR、ADX、Donchian、成交量、关键位置和 K 线形态；
- 余额、受管资产、持仓、订单、成交和浮动盈亏；
- Bybit instrument rules；
- 用户最大亏损授权；
- 原始 AI 计划和最近执行/拒绝结果。

指标是 AI 的观察工具，不是本地信号或方向过滤器。

## Superseded Scope

ADR-0005 以下内容被本 ADR 取代：

- `strategy-space-v1-vs` 是唯一可执行策略空间；
- AI 不得输出自由绝对价格和数量；
- 精确参数必须由 Deterministic Strategy Materializer 物化；
- Risk Engine 可以批准、向下调整、拒绝或降权；
- Materialization/Risk 是 Paper、Harness 和 Testnet 必经节点。

仍然有效：

- AI 不访问密钥、Exchange Adapter、工具、配置、文件系统或 Shell；
- 交易副作用只能经应用和授权 Adapter；
- 数据、账户和状态不可信时 fail closed；
- 审计、幂等、恢复、受管资产和外部事实边界；
- Testnet、实盘、永续和新闻能力需要独立授权。

## Consequences

- 发布新的 `AITradingPlan v3`，不得把 v2 `StrategyIntent` 静默映射为 v3。
- `P3-09` 取消；`P3-02` 成为历史遗留，不能进入 v3 活动链。
- v2 Risk/Materialization 代码和表由迁移 Task 安全退役，历史审计证据保留。
- AI 输出完整精确交易方案，Prompt、Context、Model、Schema、Validator 和 Execution 独立版本化。
- Paper、Historical Harness 和 Testnet 必须证明本地没有改写 AI 方案。
- Rule-only 仅作离线对照，不进入生产链。
- AI 方案可能不盈利；收益通过历史、Paper 和 Testnet 证据评估，不能由本地策略替代 AI。
- 模型错误不能绕过用户最大亏损、余额、受管资产、协议、幂等和状态边界。

## Considered Options

- **继续 v2 有界 AI + Materializer + Risk**：易于复现，但本地框架实际决定交易，不符合产品初衷。
- **AI 可输出精确计划，Risk/Materializer 仍可修改**：名义 AI 主导，实际权威仍分裂。
- **AI 完整决策，本地只接受或拒绝并忠实执行**：最大化 AI 判断能力，同时保留用户授权、协议和系统可靠性边界；本项目采用此方案。
- **AI 直接持有密钥并调用交易所**：权限不可审计、不可恢复，且扩大攻击面；不采用。
