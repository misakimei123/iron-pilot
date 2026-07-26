# IronPilot 领域词汇

IronPilot 是 AI 主导交易、确定性事实与可靠执行相结合的自治交易系统。本词汇表统一计划、代码、测试和审计语言；权限边界以 `docs/DEVELOPMENT_PLAN.md` v3 和 ADR-0006 为准，外部协议 SDK 复用边界以 ADR-0007 为准。

## 工程集成语言

**成熟 SDK 强制复用（Mandatory Mature SDK Reuse）**:
当外部标准协议已有满足功能、安全、许可证与资源边界的成熟、流行、持续维护 SDK 时，必须直接使用 SDK 的协议方法、类型与解析能力；自有代码只能实现 IronPilot 领域映射和 SDK 未提供的最小安全扩展。
_Avoid_: 薄封装为名的协议重写、自建 endpoint、wire DTO、响应 envelope、自定义轮询/分页/重试协议

## 核心交易语言

**AI 主导交易系统（AI-Dominant Trading System）**:
AI 基于完整市场、账户、订单和用户授权上下文，独立形成包含精确 entry、quantity、stop、take-profit 和管理动作的交易方案；本地系统只验证、持久化、执行、恢复和审计，不替 AI 设计或改写交易。
_Avoid_: AI 辅助传统策略、AI 审核器、有界策略模板选择器

**交易标的（Instrument）**:
由交易所、产品类型、交易符号和结算属性共同确定的可交易对象；相同 `symbol` 的现货和永续是不同标的。
_Avoid_: 币种、泛化交易对

**AI 决策上下文（AI Decision Context）**:
一次 AI 决策使用的不可变、版本化事实集合，至少包含有界原始 15m/1h OHLCV、当前价格/盘口、Market Features、K 线形态、instrument rules、余额、受管资产、持仓、订单、最近成交、用户最大亏损授权、时间和执行反馈。
_Avoid_: 指标信号、策略候选、只含 RSI/EMA 的摘要 Prompt

**AI 完整交易方案（`AITradingPlan v3`）**:
AI 对一个不可变 Context 输出的结构化完整交易决定。开仓方案包含精确 order type、entry、quantity、stop、take-profit、滑点、有效期和管理计划；持仓方案可以是 HOLD、CANCEL_ENTRY、MODIFY_PROTECTION、REDUCE 或 EXIT。
_Avoid_: Strategy Intent、Candidate Decision、AI 建议、AI 仓位建议

**交易决策权（Trading-Decision Authority）**:
是否交易以及正常交易方向、精确价格、数量、保护和退出方案的决定权。v3 中该权力属于 AI；本地组件不能生成、优化或重写这些字段。
_Avoid_: 有界策略选择权、Risk 审批权、Materializer 参数权

**决策触发（Decision Trigger）**:
仅决定何时构建 Context 并调用 AI。触发可基于 K 线闭合、信息增量、订单/成交变化或复评到期，但不能认定“有效机会”、决定方向或过滤与本地观点不一致的合法行情。
_Avoid_: Rule Prefilter、交易信号、机会筛选器

**市场特征快照（Market Feature Snapshot）**:
由连续已闭合数据确定性生成、带版本和来源证据的 RSI、EMA、ATR、ADX、Donchian、成交量、关键位置与形态观察。它是 AI 的辅助事实，不是交易指令。
_Avoid_: 买卖信号、策略输出

**原始行情上下文（Raw Market Context）**:
提供给 AI 的有界已闭合 OHLCV 序列、当前价格与盘口。它与派生指标同时提供，避免传统指标摘要限制 AI 判断。

**用户最大亏损授权（User Maximum Loss Authorization）**:
用户或部署配置允许单笔 AI 方案承担的最大 quote 资产亏损金额。它是硬账户授权，不是本地策略判断；超限方案整体拒绝，禁止本地缩量或移动止损。
_Avoid_: Risk Tier、AI 可修改风险预算、仅写在 Prompt 的建议值

**执行校验（Execution Validation）**:
对 AI 原始计划执行 Schema、时效、Spot、Bybit rules、余额、受管资产、订单冲突、最大亏损、权限、幂等和状态检查。结果只有 ACCEPT 或 REJECT；不得修改 AI 计划。
_Avoid_: Risk Engine、策略审批、ADJUST_DOWN、自动参数修复

**忠实执行（Faithful Execution）**:
TradePlan 和 Adapter 对已接受 AI 方案进行持久化、协议翻译、提交、查询、恢复和对账，交易语义与 AI 原始计划逐字段一致。
_Avoid_: 本地优化、自动止盈止损、执行层策略

**交易计划（TradePlan）**:
AI 原始交易方案从校验、订单、成交、持仓管理到关闭的持久化业务实体。它保存 AI 方案，不重新设计方案。
_Avoid_: 本地策略、信号、订单别名

**执行拒绝反馈（Validation Rejection Feedback）**:
Validator 对非法、陈旧、超授权或协议不兼容 AI 方案形成的结构化原因。Provider 最多按配置进行一次有界 replan；本地不能代替 AI 修复。

**持仓复评（AI Position Review）**:
把最新行情、账户、订单、成交、保护单和原始计划重新交给 AI，由 AI 决定 HOLD、修改保护、减仓或退出。
_Avoid_: 本地 trailing、固定复评策略、自动盈亏平衡

**风险停机（System Halt）**:
因数据、状态、授权或基础设施不可信而禁止新动作的系统状态。它保护执行可靠性，不拥有交易策略权。
_Avoid_: Risk Engine 策略裁决

**受管资产（Managed Asset）**:
能够通过 IronPilot 的 AI TradePlan、订单和成交审计链证明归属的现货数量。
_Avoid_: 子账户全部余额、可见余额

**交易所外部事实（Exchange External Truth）**:
Bybit 的订单、成交、余额和持仓是外部事实；本地数据库是 AI 原始计划、审计和恢复源。REST ack 不等于成交。

**业务幂等（Business Idempotency）**:
通过稳定 Plan/Action/Order ID、持久化、查询确认和状态机保证重复请求不产生重复业务效果。
_Avoid_: 网络 exactly-once 承诺

**紧急退出（Emergency Exit）**:
独立于 AI 和 Telegram 可用性的用户授权安全路径，只撤销可证明归属的订单并降低或关闭受管敞口；不授予本地正常策略权。

## 历史与证据语言

**市场回放（Market Replay）**:
复现相同历史输入、时钟、Market Features 和 Decision Context 输入，不计算策略 PnL，也不调用实时 LLM。

**最小历史闭环（Minimal Historical Harness）**:
使用录制 `AITradingPlan v3` 或确定性 AI Plan Stub，复用 Execution Validation、TradePlan 和 Paper Execution，证明无前视和账本可复现。活动链没有 Materializer 或 Risk Engine。

**历史策略评估（Historical Strategy Evaluation）**:
在冻结 manifest 上评估录制 AI Plan 的收益、回撤、成本、样本外和压力证据。Rule-only 仅为离线对照，不进入生产链。

**Paper Trading**:
实时行情驱动、无交易所写副作用的 AI Plan 执行模式。

**Testnet Protocol Smoke**:
极少量已授权 Bybit Testnet 写操作，用于验证协议、幂等、私有事件和对账，不代表策略资格认证。

**Bybit Testnet Qualification**:
在 Paper 和历史证据通过后，使用冻结 Context/Prompt/Model/AITradingPlan/Validator/Execution/授权进行的资格与稳定性验证。

## 已取代词汇

以下属于 v2 历史语言，不得进入 v3 活动架构：

- `strategy-space-v1-vs`
- `StrategyIntent v2`
- Deterministic Strategy Materializer
- Materialized Trade Parameters
- 后置策略型 Risk Engine
- `APPROVE / ADJUST_DOWN` 风险裁决
- Entry/Stop Anchor
- Risk Tier

v2 历史表、代码、提交和审计可以保留，但必须标记为遗留并由 `P3-12` 安全迁移。

## 新闻边界

当前默认业务链没有 NewsRiskGuard、新闻 Provider、新闻 Prompt 输入或新闻 Gate，也不声称具备新闻风险保护。未来引入新闻能力前必须先修订计划和 ADR。
