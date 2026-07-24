---
status: accepted
amended-by: 0005-bounded-ai-strategy-authority
---

# 迁移版本化市场特征与 K 线形态语义

IronPilot 以 alphaMind clean `main@1f75d21567db2cbb3dfdea831516ceb740f5b32e` 为迁移基线，迁移其中可复算且适合本项目的 Donchian、EMA、Wilder RSI/ATR/ADX、成交量比率、EMA 排列、关键位置和 11 种 K 线形态语义，但将其重新冻结为独立的 `ironpilot-market-features-v1`。迁移只复用有来源的公式、受控枚举和脱敏测试向量，不复用 alphaMind 的运行时、数据库、配置、Prompt、模型输出或交易结论，从而保留计算证据又避免跨仓故障和权限耦合。

本 ADR 的特征迁移与版本化决策保持有效。原有下游管线术语由 [ADR-0005](0005-bounded-ai-strategy-authority.md) 修订：Market Features 与 Pattern Observations 是 AI 和确定性资格门禁的只读事实；它们不能把 Eligibility / Event Prefilter 变成策略引擎，也不能替 AI 选择策略。

## Considered Options

- 只保留 EMA、RSI、ATR、ADX 等泛化名称：计划较短，但同名指标可能因播种、平滑、warm-up 和缺失值规则不同而在 Replay 与实时路径产生语义漂移。
- 直接调用或共享 alphaMind 的特征模块：减少重复代码，但会让 IronPilot 的恢复、版本和审计依赖另一个产品的运行时与发布节奏。
- 迁移计算合同并独立实现：需要 parity fixtures 和来源治理，但能在 IronPilot 内形成可恢复、可审计且不跨仓授权的唯一语义；本项目采用此方案。

## Consequences

- 15m 主决策周期和 1h 确认周期分别计算同一版本合同；不同 timeframe 的指标数值不要求相等。
- 指标、关键位置和形态只能作为 Eligibility / Event Prefilter 与 AI 的只读观察，不能直接决定交易方向、策略家族、入场、止损、目标或退出政策，也不能绕过 Strategy Intent 验证、Deterministic Strategy Materialization 或 Risk Engine。
- 开源 Rust 技术指标库仍是首选，但必须通过迁移向量与独立向量 parity；不满足时只实现缺失的最小递推并记录依赖决策。
- 任何公式、窗口、量化、形态优先级或 `null` 语义变化都必须发布新的 feature version，禁止原地改变 `v1`。
