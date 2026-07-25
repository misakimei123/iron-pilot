---
status: accepted
amended-by: 0006-ai-dominant-trading-authority
---

# 迁移版本化市场特征与 K 线形态语义

IronPilot 以 alphaMind clean `main@1f75d21567db2cbb3dfdea831516ceb740f5b32e` 为迁移基线，迁移其中可复算且适合本项目的 Donchian、EMA、Wilder RSI/ATR/ADX、成交量比率、EMA 排列、关键位置和 11 种 K 线形态语义，但将其重新冻结为独立的 `ironpilot-market-features-v1`。迁移只复用有来源的公式、受控枚举和脱敏测试向量，不复用 alphaMind 的运行时、数据库、配置、Prompt、模型输出或交易结论，从而保留计算证据又避免跨仓故障和权限耦合。

本 ADR 的特征迁移与版本化决策保持有效。下游权限由 [ADR-0006](0006-ai-dominant-trading-authority.md) 修订：Market Features 与 Pattern Observations 是 AI 的只读事实输入；触发层不得把它们变成策略信号或方向过滤器。AI 同时接收有界原始 15m/1h OHLCV 序列，避免传统指标摘要限制 AI 判断。

## Considered Options

- 只保留 EMA、RSI、ATR、ADX 等泛化名称：计划较短，但同名指标可能因播种、平滑、warm-up 和缺失值规则不同而在 Replay 与实时路径产生语义漂移。
- 直接调用或共享 alphaMind 的特征模块：减少重复代码，但会让 IronPilot 的恢复、版本和审计依赖另一个产品的运行时与发布节奏。
- 迁移计算合同并独立实现：需要 parity fixtures 和来源治理，但能在 IronPilot 内形成可恢复、可审计且不跨仓授权的唯一语义；本项目采用此方案。

## Consequences

- 15m 主决策周期和 1h 确认周期分别计算同一版本合同；不同 timeframe 的指标数值不要求相等。
- 指标、关键位置和形态只能作为 AI 的只读观察与调用触发事实；本地组件不能据此决定方向、entry、quantity、stop、target 或退出，也不能过滤掉合法、新鲜但与本地规则观点不一致的 Context。
- 开源 Rust 技术指标库仍是首选，但必须通过迁移向量与独立向量 parity；不满足时只实现缺失的最小递推并记录依赖决策。
- 任何公式、窗口、量化、形态优先级或 `null` 语义变化都必须发布新的 feature version，禁止原地改变 `v1`。
