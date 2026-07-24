# IronPilot AI 策略权限修订方案

> 文档状态：`PROPOSED_CHANGE_SPEC`
>
> 日期：2026-07-24
>
> 适用范围：`docs/DEVELOPMENT_PLAN.md`、ADR-0002、AI Schema、Rule Prefilter、Trade Parameters、TradePlan、Backtest 与 Release Gate
>
> 说明：本文件用于冻结产品方向和修改要求。在 `docs/DEVELOPMENT_PLAN.md` 完成同步前，现有开发计划仍是任务状态与依赖的权威来源；但任何 AI、策略预筛、交易参数或回测实现不得与本修订方向冲突。

---

## 1. 修订背景

IronPilot 的核心目标不是构建一个“传统规则策略加自动下单”的量化系统，也不是让大模型直接控制交易账户，而是验证：

> AI 能否在不可绕过的确定性风险、权限、状态和执行边界内，持续形成可恢复、可审计的交易策略并自动执行。

当前设计已经正确建立了以下安全边界：

- AI 不接触 API Key、交易所 Adapter、文件系统或 Shell。
- AI 无权修改风险配置、运行模式、标的权限或账户设置。
- 下单、对账、恢复、幂等、资金安全和紧急退出由确定性系统负责。
- 交易参数必须经过本地校验、Risk Engine 审批和执行前检查。
- 数据、账户、订单或状态不可信时 fail closed。

但当前业务主链存在产品性质漂移风险：

```text
技术指标与 K 线形态
→ Rule Prefilter 识别“有效交易机会”
→ AI 只判断是否同意
→ 本地代码自行决定入场、止损、止盈和持仓管理
→ Risk Engine
→ Execution
```

若 Rule Prefilter 已经决定哪些场景值得交易，而 Trade Parameters Calculator 又独立生成完整交易方案，则 AI 的实际职责只剩“对规则信号盖章”。最终产品将更接近：

> alphaMind 式确定性分析逻辑 + LLM 二次过滤 + 自动执行。

这无法充分验证 IronPilot 最初的 AI 受限自治假设。

---

## 2. 修订后的核心原则

### 2.1 一句话原则

> **不要让 AI 控制账户，但必须让 AI 真正控制策略。**

英文规范：

> **AI has bounded strategy authority, but no execution authority and no authority to override deterministic risk constraints.**

中文规范：

> **AI 在预授权、版本化且可验证的策略空间内拥有候选交易计划的选择权，但不拥有执行权限，也无权覆盖确定性风险边界。**

### 2.2 三种权限必须严格分离

| 权限 | 权威组件 | AI 是否拥有 |
|---|---|---:|
| 策略权限 | AI Strategy Decision Provider | 在受控策略空间内拥有 |
| 风险权限 | Deterministic Risk Engine | 否 |
| 执行权限 | TradePlan + Execution + Exchange Adapter | 否 |

AI 可以决定“想如何交易”；确定性系统决定“最多允许如何交易”；执行系统负责“安全地完成交易”。

---

## 3. 修订后的权威业务主链

```text
Bybit REST / WebSocket 行情与账户事实
→ 版本化 Market Features 与受控 Pattern Observations
→ Eligibility / Event Prefilter
→ News Risk Guard（veto-only）
→ AI Strategy Intent
→ Strategy Intent Schema / Semantic Validation
→ Deterministic Strategy Materialization
→ Risk Engine 审批或收紧
→ 持久化 TradePlan
→ Execution Preflight
→ Bybit API 执行与私有流 / REST 对账
→ Telegram 通知已确认结果
```

关键变化：

1. Rule Prefilter 不再产生隐含的交易结论，只判断是否具备调用 AI 的数据、成本和事件资格。
2. AI 不再只是输出市场语义和简单动作，而是选择受控的交易策略、入场政策、失效政策、退出政策、持有期限和复评方式。
3. 确定性参数模块不再自行设计交易，而是把 AI 已选择的合法策略意图物化成精确价格、数量和订单参数。
4. Risk Engine 仍然只有拒绝或收紧权限，不能替 AI 换成另一种策略，也不能扩大风险。

---

## 4. 组件职责重新划分

## 4.1 Market Features：提供可信观察，不产生交易结论

保留从 alphaMind 迁移并独立版本化的：

- Donchian
- EMA
- Wilder RSI / ATR / ADX
- 成交量比率
- EMA 排列
- 关键位置
- 受控 K 线形态与语义
- 15m 主周期与 1h 确认周期

这些字段是 AI 和本地门禁的只读事实，不具备开仓、平仓或反转权限。

## 4.2 Rule Prefilter 改为 Eligibility / Event Prefilter

Rule Prefilter 不应成为隐藏策略引擎。

### 允许负责

- K 线是否闭合、连续和未过期。
- 指标是否完成 warm-up。
- Market Feature Snapshot 是否可信。
- 价差、流动性和数据质量是否达到最低要求。
- 是否处于标的或全局冷却期。
- 是否已有互斥的活动 TradePlan。
- 是否超过 LLM 调用、Token 或成本预算。
- 市场是否出现值得重新决策的结构变化或持仓复评事件。
- 对重复、低信息量事件进行去重和限流。

### 可以生成的事件类型示例

- `STRUCTURE_CHANGED`
- `KEY_LOCATION_REACHED`
- `VOLATILITY_EXPANDED`
- `VOLUME_ANOMALY`
- `BREAKOUT_ATTEMPT`
- `RETEST_EVENT`
- `POSITION_REVIEW_DUE`
- `INVALIDATION_RISK_INCREASED`

### 不允许负责

- 直接断言这是“有效做多机会”或“有效卖点”。
- 用 EMA、RSI、ADX、Donchian 或形态组合直接生成最终交易方向。
- 固定决定必须使用何种入场、止损或止盈方案。
- 因本地策略偏好替 AI 排除所有其他合法策略家族。

预筛目标应从“过滤至少 90% 非候选交易场景”改为：

> 在不预先替代 AI 策略判断的前提下，过滤数据不可信、状态不合法、重复、无新增信息或不值得付出 LLM 成本的事件。

过滤率仍可作为成本观测指标，但不得成为诱导实现者把策略逻辑塞入 Prefilter 的硬目标。

---

## 5. AI Strategy Intent 协议

## 5.1 AI 应拥有的策略选择权

Spot MVP 中，AI 可以在版本化白名单中选择：

- 是否开仓、持有、减仓、退出或继续观察。
- 策略家族。
- 入场政策。
- 止损与失效政策。
- 目标与退出政策。
- 风险档位。
- 最大等待时间与最大持有时间。
- 下一次复评触发条件。
- 哪些受控市场变化会使原交易假设失效。

## 5.2 AI 仍然不得拥有的权限

AI 输出不得包含或控制：

- API Key、账户、子账户或 Exchange Adapter。
- 任意绝对仓位数量或账户资金百分比。
- 任意杠杆值、保证金模式或持仓模式。
- 任意绕过本地锚点的自由价格。
- 交易所订单 ID、幂等键或执行重试。
- 日损、回撤、组合敞口和最大风险上限。
- 配置文件、Prompt、模型、标的启用状态或运行模式。
- Shell、文件系统、网络工具或 MCP 交易调用。

## 5.3 建议 Schema v2

以下仅冻结语义边界，具体字段名称由实现 Task 最小化收敛：

```json
{
  "schema_version": "2.0",
  "decision_id": "uuid",
  "snapshot_id": "uuid",
  "instrument_id": "bybit:spot:BTCUSDT",
  "action": "NO_TRADE|OPEN_LONG|HOLD|REDUCE|EXIT",
  "strategy_family": "trend_breakout|trend_pullback|range_reversion|defensive_exit|none",
  "entry_policy": {
    "type": "immediate_confirmed|breakout_retest|pullback_to_anchor|limit_at_anchor|none",
    "anchor": "donchian_upper|donchian_lower|ema_fast|ema_slow|key_location|recent_swing|none",
    "max_wait_bars": 2,
    "confirmation": "close_confirmed|rejection_confirmed|volume_confirmed|none"
  },
  "stop_policy": {
    "type": "structure_with_atr_buffer|atr_only|time_invalidation|none",
    "anchor": "recent_swing|key_location|ema_slow|donchian_opposite|none",
    "buffer_tier": "tight|normal|wide|none"
  },
  "target_policy": {
    "type": "next_structure|fixed_rr_tier|trailing|partial_then_trailing|none",
    "minimum_rr_tier": "1_5R|2R|3R|none",
    "trailing_anchor": "ema_fast|ema_slow|atr_band|structure|none"
  },
  "risk_tier": "conservative|normal",
  "maximum_holding_bars": 12,
  "review_policy": "every_primary_close|on_structure_change|on_invalidation_risk|combined",
  "invalidation_conditions": [
    "breakout_failed",
    "trend_confirmation_lost"
  ],
  "market_regime": "trend|range|breakout|uncertain",
  "confidence": "0.00..1.00",
  "thesis": "short bounded text",
  "data_quality_assessment": "acceptable|insufficient",
  "risks": []
}
```

所有枚举、组合关系、长度、TTL 和动作合法性均由本地 Schema、Serde 和语义验证器严格校验，未知字段默认拒绝。

## 5.4 策略模板必须版本化

每个策略家族和策略政策必须绑定：

- `strategy_space_version`
- 允许的产品类型
- 允许的 action
- 合法 anchor
- 合法 policy 组合
- 默认失败语义
- 最大等待与持有范围
- 允许的风险档位

修改策略空间必须发布新版本，不得在版本不变时静默改变枚举含义。

---

## 6. Trade Parameters Calculator 的职责修订

建议将概念职责改为：

- `StrategyIntentMaterializer`
- 或 `BoundedTradePlanCompiler`

代码模块可以暂时保留 `trade_parameters` 名称，但文档必须明确它是“物化器”，不是第二策略引擎。

## 6.1 输入

- 已通过严格校验的 `AI Strategy Intent`
- 与其绑定的不可变 Market Feature Snapshot
- Portfolio Snapshot
- Instrument Constraints
- Risk Config
- Strategy Space Version
- Materialization Algorithm Version

## 6.2 允许负责

- 校验 AI 选择的锚点在当前快照中真实存在。
- 将 `donchian_upper`、`recent_swing`、`key_location`、EMA 等锚点转换为精确价格。
- 根据 AI 选择的 buffer tier 计算受限 ATR buffer。
- 根据 AI 选择的目标政策计算候选目标和移动退出规则。
- 应用手续费、滑点、tickSize、qtyStep 和最小金额。
- 根据风险预算计算不超过上限的最大允许数量。
- 生成执行前仍需 Risk Engine 审批的确定性 `TradeParameters`。

## 6.3 不允许负责

- 在 AI 选择 `breakout_retest` 后擅自改成 `immediate_confirmed`。
- 在 AI 选择结构止损后擅自换成纯 ATR 止损。
- 在目标不满足最低盈亏比时替 AI 降低门槛。
- 当 AI 策略无法物化时自行选择另一策略继续交易。
- 根据 AI confidence 扩大仓位。
- 生成超出确定性风险上限的数量。

核心不变量：

> **确定性代码可以拒绝或收紧 AI 的计划，但不能悄悄替 AI 重新设计一笔交易。**

无法合法物化时，结果必须是 `REJECTED` 或 `NO_TRADE`，并记录明确原因。

---

## 7. Risk Engine 与执行安全边界保持不变

本修订不削弱现有安全原则。

Risk Engine 继续负责：

- 系统状态和数据可信度。
- 单笔风险、标的敞口、风险组敞口和组合敞口。
- 日损、周损、回撤、连续亏损和冷却。
- 最大活动计划与持仓数量。
- 最低可用资金、流动性、价差和滑点。
- 交易所动态约束。
- 止损和保护条件是否可建立。
- 任何不可信状态下的 `REDUCE_ONLY` / `HALT`。

Risk Engine 只能：

- `APPROVE`
- `ADJUST_DOWN`
- `REJECT`
- `REDUCE_ONLY`
- `HALT_SYMBOL`
- `HALT_SYSTEM`

Risk Engine 不得：

- 将被拒绝策略替换为另一策略。
- 改变 AI 的交易方向。
- 扩大 AI 申请的风险档位。
- 因历史收益、AI confidence 或新闻利好放宽硬边界。

Execution 仍然只消费已持久化、已审批且未过期的 TradePlan 操作，并保持幂等、未知结果冻结、真实状态对账和紧急退出语义。

---

## 8. 持仓管理中的 AI 策略权限

AI 的策略权限不应只存在于首次开仓。

对已有 TradePlan，AI 可以在预授权范围内输出：

- `HOLD`
- `REDUCE`
- `EXIT`
- 保持原策略政策
- 在同一策略模板允许范围内收紧失效条件或退出政策

AI 不得：

- 放宽原始最大风险。
- 下移止损以扩大亏损空间。
- 对亏损仓位执行 Martingale 或无上限补仓。
- 在未经新 TradePlan 与新风险审批时反向开仓。
- 因主观叙事推翻交易所真实状态和本地对账结果。

任何扩大风险的变更都必须被视为新的候选交易计划，重新经过完整物化、Risk Engine 和执行前检查；Spot MVP 默认可直接禁止此类操作。

---

## 9. 回测必须验证 AI 的增量价值

IronPilot 的回测不能只回答“这套整体系统是否盈利”，还必须回答：

> 相同市场事实、风险约束、参数物化和执行模型下，AI 是否提供了可衡量的增量价值？

## 9.1 三个强制对照组

同一个不可变 `BacktestManifest` 下至少运行：

### A. Rule-only Baseline

- 由最小公开、冻结的确定性基线策略直接选择交易。
- 不调用 LLM。
- 用于衡量市场特征与基础策略本身的表现。

### B. Deterministic Decision Stub

- Eligibility Prefilter 产生相同事件。
- 使用固定、可复现的 Decision Stub 选择策略意图。
- 用于验证编排、物化、Risk、TradePlan 和 Execution，而不引入模型随机性。

### C. AI Strategy Decision

- 使用录制的 DeepSeek 决策或在受控实验中生成并冻结决策。
- 其他输入、风险、物化和执行条件与 A/B 保持可比。

## 9.2 必须比较的指标

除现有收益和风险指标外，增加：

- 相对 Rule-only 的净收益增量。
- 相对 Rule-only 的最大回撤变化。
- 单笔期望变化。
- 交易次数与事件转化率。
- AI 放弃规则基线交易后的机会成本。
- AI 新选择策略家族的贡献和损失。
- Risk Engine 拒绝率与拒绝原因。
- 无法物化的 AI Strategy Intent 比率。
- Schema / Semantic failure 比率。
- 每个有效候选的 Token 和成本。
- 每 1 USDT 增量收益对应的模型成本。
- 不同 market regime 下的 AI 增量表现。

## 9.3 升级规则

- AI 表现弱于 Rule-only 不代表工程失败，但该 AI 策略版本不得自动升级为 `entry_enabled`。
- AI 只在部分 market regime 有增量时，应限制其策略权限范围，而不是用总体平均掩盖分段失效。
- 任何安全不变量失败都不能被盈利抵消。
- 单次高收益、短期胜率或高 confidence 不能证明 AI 有价值。

---

## 10. 对现有开发计划的具体修改清单

同步 `docs/DEVELOPMENT_PLAN.md` 时至少修改以下位置。

### 10.1 第 1、2、5 节：项目定位和不可妥协原则

将宽泛的：

```text
AI has no authority
```

修改为：

```text
AI has bounded strategy authority, but no execution or risk-override authority.
```

同时保留 AI 无密钥、无工具、无配置和无风险覆盖权限。

### 10.2 第 5.3 节：权威业务主链

替换为第 3 节定义的新主链，并将：

- `Rule Prefilter` 改为 `Eligibility / Event Prefilter`
- `CandidateDecision` 升级为 `AI Strategy Intent`
- `Trade Parameters Calculator` 明确为 `Deterministic Strategy Materialization`

### 10.3 第 9 节：数据流

明确 Prefilter 只负责资格、数据、事件、去重和预算，不负责最终策略选择。

### 10.4 第 10、14 节：领域模型与 AI 协议

新增或修改：

- `StrategyIntent`
- `StrategyFamily`
- `EntryPolicy`
- `StopPolicy`
- `TargetPolicy`
- `RiskTier`
- `ReviewPolicy`
- `StrategySpaceVersion`

AI Schema 从 v1 升级为 v2，旧 v1 只允许用于历史回放，不得静默映射到 v2 后进入生产。

### 10.5 第 15 节：交易参数

将 Trade Parameters Calculator 的说明从“自行计算完整策略”改为“将 AI 选择的合法策略政策物化为确定性执行参数”。

### 10.6 `P2-03`

验收标准增加：

- Prefilter 不包含决定最终交易方向的隐藏策略规则。
- 任一事件是否调用 AI 的决定可由数据质量、状态、信息增量和预算解释。
- 删除“为了达到 90% 过滤率而压制合法策略空间”的激励。

### 10.7 `P3-04`

DeepSeek Provider 改为产生严格的 `StrategyIntent v2`，测试必须覆盖：

- 非法策略组合。
- 不存在的锚点。
- 自由绝对价格、数量或杠杆注入。
- 试图修改风险边界。
- 策略模板越权。
- 持仓管理中扩大风险。

### 10.8 `P3-09`

重命名任务语义为“确定性策略物化与交易参数”，并增加不变量：

- 相同 Strategy Intent 与输入产生相同参数。
- 无法物化时不得改用其他策略。
- 所有精确价格均可追溯到 AI 选择的受控 anchor 和本地算法版本。

### 10.9 `P3-10`

增加第 9 节三组对照实验和 AI 增量价值报告。

### 10.10 `P3-06`

闭环名称修改为：

```text
Market Features
→ Eligibility Event
→ News Guard
→ AI Strategy Intent
→ Intent Validation
→ Deterministic Materialization
→ Risk
→ TradePlan
→ Paper Execution
→ Position Review / Exit
```

### 10.11 第 28 节 Gate

新增硬指标：

| 指标 | 目标 |
|---|---:|
| 未经合法 Strategy Intent 产生的订单 | 0 |
| AI 自由绝对数量、价格或杠杆进入执行 | 0 |
| Materializer 擅自替换 AI 策略 | 0 |
| 不存在锚点仍生成交易参数 | 0 |
| AI 扩大已有 TradePlan 风险成功 | 0 |
| Strategy Intent 可复现与追溯率 | 100% |
| A/B/C 对照报告覆盖率 | 100% |

---

## 11. 版本和迁移策略

1. 发布 `strategy-space-v1`，只包含 Spot MVP 实际支持的最小策略家族和政策。
2. 发布 `candidate-decision-schema-v2` 或等价命名。
3. 旧 Schema v1 决策只可用于历史审计和 Replay，不可静默升级进入 Paper/Testnet。
4. Prompt、Schema、Strategy Space、Materialization Algorithm 和 Risk Rules 必须分别版本化并记录 hash。
5. 同一 TradePlan 必须可追溯：

```text
Market Snapshot
→ Eligibility Event
→ Prompt / Model
→ Strategy Intent
→ Strategy Space Version
→ Materialization Version
→ Risk Decision
→ Order Intent
→ Order / Fill
```

6. 任何策略枚举、锚点、组合规则或风险档位语义变化都必须发布新版本。

---

## 12. 推荐实施顺序

1. 接受 ADR-0005，冻结“有界 AI 策略权限”。
2. 修改 `docs/DEVELOPMENT_PLAN.md` 的主链、原则、Schema、Task 和 Gate。
3. 在 `P1-02` 中增加 Strategy Intent 纯领域类型和组合不变量。
4. 在 `P2-03` 中将 Rule Prefilter 收缩为 Eligibility / Event Prefilter。
5. 在 `P3-04` 中实现 Strategy Intent v2。
6. 在 `P3-09` 中实现确定性 Strategy Materializer。
7. 在 `P3-03/P3-06` 中支持持仓期间的受控 AI 复评。
8. 在 `P3-10` 中实现 Rule-only、Decision Stub 和 AI 三组对照实验。
9. 在进入长期 Paper 和 Testnet Gate 前，验证 AI 确实拥有策略选择权，但无法越过风险和执行边界。

---

## 13. 验收结论

完成本修订后，IronPilot 应满足以下产品定义：

- alphaMind 提供可复算的市场特征和形态语义迁移基础。
- Eligibility Prefilter 只决定是否值得调用 AI，不替 AI 完成交易策略。
- AI 在受控策略空间内选择真正影响交易结果的策略政策。
- 确定性物化器将 AI 选择转换为精确、可复现、可审计的交易参数。
- Risk Engine 可以拒绝或缩小风险，但不能替 AI 重新设计交易。
- Execution 只负责安全、幂等和可恢复地执行已批准 TradePlan。
- 用户不逐笔审批，只保留通知、查询和紧急退出能力。
- 回测能够单独衡量 AI 相对规则基线的增量价值。

最终边界：

> **IronPilot 不是让 AI 随意交易，而是让 AI 在围栏内真正做交易决策。**
