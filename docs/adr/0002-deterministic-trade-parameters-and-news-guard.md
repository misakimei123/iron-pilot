---
status: accepted
---

# 确定性交易参数与新闻风险守卫

IronPilot 的本地规则先从 Bybit 行情计算 EMA、RSI、ATR、ADX、成交量、价差和市场结构，并过滤绝大多数无交易价值的场景；只有规则候选通过结构化 News Risk Guard 后才调用 LLM。LLM 只判断市场语义和交易意图，不产生可执行仓位、止损或止盈；最终交易参数由确定性代码计算并由 Risk Engine 审批。生产订单只通过 Bybit REST/Private WebSocket API Adapter 执行，MCP 不属于运行时交易权限边界。

## Considered Options

- 由 LLM 直接给出仓位、止损和止盈：更灵活，但难以保证可复现、边界正确和风险规则不可绕过。
- 使用低成本 LLM 阅读新闻并充当守卫：覆盖自由文本，但仍存在 Prompt Injection、成本和非确定性。
- 使用结构化新闻/事件源和确定性 veto-only 规则：能力边界较窄，但可审计、低成本且不会产生交易权限；本项目采用此方案。

## Consequences

- 新闻守卫失效或数据过期时默认禁止新的 AI 开仓，已有持仓继续由确定性保护管理。
- “黑天鹅”只能作为风险目标，不能作为可保证检测的能力或验收承诺。
- LLM Schema 只保留语义候选字段；交易数量、止损和止盈不接受模型输出。
- 回放和 Paper Trading 必须包含版本化新闻事件与确定性交易参数计算。
