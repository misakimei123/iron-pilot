# IronPilot 领域词汇

IronPilot 是受确定性权限、风险边界和状态一致性约束的自动交易系统。本词汇表统一后续计划、代码、测试和审计记录中的业务语言。

## 核心交易语言

**受限自治交易系统（Constrained Autonomous Trading System）**:
在预先授权的交易范围内自动产生并执行交易，但任何动作都必须服从确定性风控、状态机和审计约束的系统。
_Avoid_: 自主交易员、无限自治 Agent

**交易标的（Instrument）**:
由交易所、产品类型、交易符号和结算属性共同确定的可交易对象；相同 `symbol` 的现货和永续合约是不同交易标的。
_Avoid_: 币种、交易对（用于泛指不同产品时）

**策略意图（Strategy Intent）**:
AI 针对一个交易标的和不可变市场上下文，在版本化 Strategy Space 内选择的结构化策略；它具有有界策略权限，但没有风险覆盖或执行权限。`Candidate Decision` 是历史术语，不用于当前运行时合同。
_Avoid_: AI 指令、最终执行决策、Candidate Decision

**Vertical Slice 策略空间（`strategy-space-v1-vs`）**:
`P3-VS` 前唯一可进入 DeepSeek 真实输出校验、Materializer、Risk Engine、TradePlan、Paper Runtime 和 Minimal Historical Harness 的可执行 Strategy Space 版本。完整 `StrategyIntent v2` Schema 中的未来枚举只是协议边界参考；后续能力必须发布新版本，不得静默扩展本版本。
_Avoid_: `strategy-space-v1`（作为 P3-VS 运行时版本）、完整 Schema 等于当前可执行集合

**资格与事件预筛（Eligibility / Event Prefilter）**:
只判断市场数据、事件、系统状态、去重、冷却和 LLM 预算是否允许发起一次 AI 策略决策；可以生成受控事件，但不能决定交易方向、策略家族、入场、止损、目标或退出政策。`Rule Prefilter` 是历史术语，不用于当前运行时合同。
_Avoid_: 规则候选生成器、有效做多机会、Rule Prefilter

**风险裁决（Risk Decision）**:
确定性 Risk Engine 对已验证 Strategy Intent、物化参数和当前账户状态作出的约束结果，可批准、拒绝、向下调整或触发受限状态；它不能替换 AI 策略或扩大风险。
_Avoid_: 风险建议、AI 风控

**新闻能力（当前非目标）**:
当前默认业务链没有 `NewsRiskGuard`、新闻 Provider、新闻 Prompt 输入或 `disabled` 占位节点，也不声称具备新闻风险保护。未来引入任何新闻门禁前，必须先修订开发计划和相关 ADR，冻结权限、失败语义、数据合同、回放证据与 Gate。
_Avoid_: 默认新闻门禁、黑天鹅探测器、新闻交易策略、新闻 AI 审批器

**市场特征快照（Market Feature Snapshot）**:
针对一个交易标的和一个 K 线周期，由连续已闭合市场数据确定性生成并带版本、时效和来源证据的一组数值与受控语义观察。
_Avoid_: 实时信号、AI 行情结论、跨周期指标包

**关键位置（Key Location）**:
由版本化市场结构规则识别的支撑、阻力或无关键位置状态；它是形态过滤条件，不是模型主观绘制的价位。
_Avoid_: AI 支撑位、主观压力位

**形态观察（Pattern Observation）**:
只在合法关键位置由确定性规则识别的可选 K 线形态及其受控语义；它本身没有开仓、平仓或反转权限。
_Avoid_: K 线信号、形态指令、必然反转

**确定性策略物化（Deterministic Strategy Materialization）**:
把已验证 Strategy Intent 中由 AI 选择的 strategy family、anchor、entry/stop/target policy 和 risk tier，结合不可变市场快照、组合状态、风险配置与交易所约束，版本化地转换为精确候选价格、数量和订单参数。物化器可以拒绝或收紧，但不能替换策略或在失败时另选交易。
_Avoid_: 第二策略引擎、规则交易方案生成器、Trade Parameters Calculator（作为领域职责）

**物化交易参数（Materialized Trade Parameters）**:
确定性策略物化的候选输出，仍须经过 Risk Engine 审批、持久化 TradePlan 和 Execution Preflight 才能产生业务副作用。代码模块可暂时保留 `trade_parameters` 名称，但该名称不授予策略选择权。
_Avoid_: AI 仓位建议、AI 止损价格、最终执行指令

**交易计划（TradePlan）**:
一次交易意图从候选、审批、入场、持仓管理到关闭与复盘的持久化业务实体。
_Avoid_: 信号、订单（订单只是 TradePlan 的执行记录）

**受管资产（Managed Asset）**:
能够通过 IronPilot 的 TradePlan、订单和成交审计链证明归属的现货数量。
_Avoid_: 子账户全部余额、可见余额

**历史策略回测（Historical Strategy Backtest）**:
在冻结的历史行情与不可变 manifest 上，复用 IronPilot 的 Market Features、Eligibility/Event Prefilter、录制 Strategy Intent 或确定性决策桩、Strategy Materialization、Risk、TradePlan 和 Paper Execution 语义，生成可复现交易账本、权益曲线和绩效证据的离线过程。当前合同不要求新闻数据。
_Avoid_: 历史回放、Paper Trading、盈利证明

**独立回测参考（Independent Backtest Reference）**:
使用与 IronPilot 不同的成熟开源实现，对冻结策略子集做离线交叉计算，以暴露指标、成交、费用和绩效语义漂移；它既不是无误的真值，也不参与生产运行。
_Avoid_: Oracle、生产回测器、第二订单权威

## 状态与安全语言

**可信状态（Trusted State）**:
本地订单、成交、余额和持仓与交易所真实状态完成对账，且行情、时钟和连接满足新开仓门槛的系统状态。
_Avoid_: 服务在线、WebSocket 已连接

**只减仓（Reduce Only）**:
系统只允许降低已有风险敞口，不允许创建或扩大任何敞口的运行约束。
_Avoid_: 暂停（暂停可能禁止一切交易）

**风险停机（Risk Halt）**:
因确定性风险阈值或状态不可信而禁止新开仓，并要求完成恢复检查后才能解除的系统状态。
_Avoid_: 临时错误、自动重试状态

**紧急退出（Emergency Exit）**:
入口 Adapter 完成自身认证和用户确认并构造 `AuthorizedEmergencyCommand` 后，由统一 EmergencyController 禁止新开仓、撤销冲突订单、按交易所真实状态降低受管敞口并最终对账的幂等流程。
_Avoid_: 撤销全部订单、卖出全部余额

**已授权紧急命令（Authorized Emergency Command）**:
Telegram、受保护 CLI 或 loopback 管理 API 在完成入口身份验证、权限检查、防重放与确认后构造的统一命令；至少携带请求、主体、来源、范围、认证/确认凭据引用和有效期语义。Emergency Core 仍验证 TTL、业务幂等和请求范围。
_Avoid_: Telegram callback、未经确认的 EmergencyAction、平台特定消息 DTO

**紧急控制器（EmergencyController）**:
不依赖 Telegram、Bot Token、入口白名单、nonce 或交互状态的统一领域/应用服务；负责稳定 EmergencyActionId、业务幂等、受管资产边界、撤单/降敞口、持久化、恢复、审计和最终结果。所有入口调用同一 Controller。
_Avoid_: Telegram Emergency Service、CLI 专用退出逻辑、按入口复制紧急业务

## 交付语言

**现货 MVP（Spot MVP）**:
以 AI 驱动的多标的现货闭环为范围，完成历史回放、历史策略回测、实时 Paper Trading、Testnet Protocol Smoke、Testnet Qualification Setup、72 小时 Stability and Recovery，并通过 Bybit Testnet Qualification Gate；不包含真实资金和永续合约。
_Avoid_: 完整产品、实盘版本

**Testnet Qualification Setup**:
`P4-02B` 在 Protocol Smoke、Long-running Paper 和 Historical Strategy Evidence Gate 通过后冻结资格测试配置、版本、回滚和停止条件，为 `P4-03` 准备环境；它不表示 Testnet 资格已经通过。
_Avoid_: Testnet Qualification、测试网已验收

**Release Gate**:
从一个风险阶段进入下一阶段前必须由独立证据和明确授权共同满足的放行门槛。
_Avoid_: 里程碑完成、默认继续
