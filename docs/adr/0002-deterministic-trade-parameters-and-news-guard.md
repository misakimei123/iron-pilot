---
status: superseded
superseded-by: 0005-bounded-ai-strategy-authority
---

# 确定性交易参数与新闻风险守卫（已取代）

本 ADR 的原始业务链把本地规则作为交易候选选择器，把结构化新闻守卫作为调用 LLM 的前置门禁，并把 LLM 限定为对本地交易方案进行语义判断。该职责划分已被 [ADR-0005](0005-bounded-ai-strategy-authority.md) 取代，不再是当前实现依据。

被取代的范围包括：

- 由本地规则预先决定“交易候选”或方向；
- `Rule Prefilter` 作为隐藏策略引擎；
- 新闻步骤作为默认业务链、Replay、Paper 或 Backtest 的前置要求；
- LLM 只批准或拒绝由本地代码设计的完整交易；
- `Trade Parameters Calculator` 独立选择入场、止损、目标或退出策略。

仍然有效的安全约束已由 ADR-0005 重新确认：

- AI 不得输出自由绝对价格、任意数量、杠杆、账户、订单 ID 或风险上限；
- 精确价格、数量和订单参数由确定性 Strategy Materializer 从合法 Strategy Intent 物化；
- Risk Engine 只能批准、收紧、拒绝或降权，不能扩大风险或替换策略；
- 生产订单只能经授权的 Exchange Adapter 执行，MCP 不属于运行时交易权限边界。

当前默认业务链明确不包含新闻能力，也不使用 `disabled` 新闻占位节点。未来如需引入新闻风控，必须先修订开发计划和相关 ADR。
