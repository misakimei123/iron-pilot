---
status: accepted
amended-by: 0006-ai-dominant-trading-authority
---

# 采用组合式历史回测与独立参考

Rust 生态已经存在 NautilusTrader、Barter 等成熟的事件驱动交易和回测基础设施，但目前没有一个可直接等价替换 Freqtrade、同时面向加密货币个人量化交易者并开箱覆盖数据下载、策略回测、参数优化、Paper/Live、Telegram/WebUI 和分析报告的一体化框架。直接选择任一框架作为 IronPilot 的第二运行时，会复制本项目的 AITradingPlan、Execution Validation、TradePlan、受管资产和订单状态机语义；完全自研 Freqtrade 等价物又会重复实现大量成熟能力。

IronPilot 因此采用组合式方案：历史策略回测必须复用本项目自身的 Market/Account Context、录制 `AITradingPlan v3` 或确定性 AI Plan Stub、Execution Validation、TradePlan 和 Paper Execution 领域链。当前默认链和回测 manifest 不包含新闻步骤。`P3-10A` 只建立 P3-VS 所需的最小历史正确性证据；`P3-10B` 在 P3-VS 后完成对照、样本外、成本压力和必要的独立参考。合格的开源能力可以窄 adapter 接入；若候选不能满足领域语义或 2C2G 资源预算，只补确定性时钟、编排和报告等最小缺口，不自研通用交易框架。Freqtrade 仅在无密钥、无交易网络的开发环境中充当离线 Rule-only 独立参考，不进入生产依赖、配置、数据库或订单链路。

本 ADR 的组合式回测与独立参考决策保持有效。活动管线和 AI 权限边界由 [ADR-0006](0006-ai-dominant-trading-authority.md) 修订。

## Considered Options

- 全面采用 Freqtrade：加密货币工作流最完整，但 Python 策略与 IronPilot Rust 领域链会形成双实现，生产订单、风险和恢复权威难以保持唯一。
- 全面采用 NautilusTrader 或 Barter：能快速获得大量成熟能力，但两者都不是 IronPilot 领域合同或 Freqtrade 的无缝替代；依赖、许可证、资源和执行语义仍需实证。
- 从零复刻 Freqtrade：可完全定制，但会把项目扩大为通用量化平台，延迟安全交易闭环并增加长期维护面。
- 复用 IronPilot 领域链并组合开源能力：保留唯一生产语义，同时通过候选框架与独立参考减少自研和暴露偏差；本项目采用此方案。

## Consequences

- Market Replay、Historical Strategy Backtest 和 Real-time Paper 是三类独立证据，必须分别通过门禁。
- 回测框架不得持有生产凭据、访问交易端点、调用实时 LLM、修改生产配置或成为订单权威。
- 成交、费用、滑点、数据切分、基准和绩效指标必须版本化并绑定不可变 manifest；跨引擎差异必须逐笔解释。
- Freqtrade 参考结果不是 Oracle；一致结果不能证明盈利，不一致结果在解释前阻止策略升级。
- 自动 Hyperopt、自动改写风险参数和回测后自动发布不属于 Spot MVP。
