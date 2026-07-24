---
status: accepted
---

# 现货优先的 AI 驱动 MVP

IronPilot 的首个纵向闭环采用 AI 驱动的多标的现货交易，并以 Bybit 测试网验收作为 MVP 终点；USDT 本位线性永续合约移到 MVP 后的独立阶段，极小规模实盘另设 Release Gate。这样可以尽早验证 AI 决策价值，同时把杠杆、保证金、强平和 Reduce-Only 的额外风险隔离在后续阶段。

## Considered Options

- 同时交付现货与永续合约：覆盖面更完整，但会把两套资产语义和杠杆风险带入第一个闭环。
- 先做纯确定性安全底座、后接 AI：风险最低，但不能尽早验证项目最核心的 AI 驱动假设。
- 现货 AI 闭环优先：保留 AI 价值验证，同时用无杠杆现货降低首个闭环的不可控风险；本项目采用此方案。

## Consequences

- `InstrumentType::LinearPerpetual`、仓位和杠杆模型必须在架构中预留，但不属于 Spot MVP 的完成条件。
- Spot MVP 完成不授权实盘；测试网、实盘和资金扩容分别使用独立门禁。
- 后续合约阶段不得复用“余额即持仓”的现货假设。
