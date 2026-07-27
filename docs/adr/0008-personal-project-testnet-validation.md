---
status: accepted
date: 2026-07-27
amends:
  - 0001-spot-first-mvp
---

# 采用个人项目定位与单次 30 天 Testnet 端到端验证

## Context

IronPilot 的目标是快速验证 AI 主导自动交易是否可行，由单个用户建设和运行。它不是金融机构级平台，也不需要用形式化混沌工程、100% 故障覆盖、on-call 体系或繁琐资格认证证明组织级可靠性。项目不设置名义资金门槛；真实资金权限仍须逐次明确授权。

原计划把后期运行证据拆成三个阶段：

1. `P3-11` 独立 30 天本地 Paper Safety Gate；
2. `P4-02B` Testnet Qualification Setup；
3. `P4-03` 独立 72 小时 Testnet Stability and Recovery。

`P3-11` 的既有实现进一步冻结了五分钟最大证据间隔、六项故障演练和 `collecting` / `qualified` / `disqualified` 资格状态。该设计能够形成严格、确定性的长期运行证据，但重复验证周期较长，并把个人可行性实验扩张为接近机构级资格流程。Paper 也无法替代 Bybit Testnet 私有订单、成交和对账的真实协议链。

## Decision

### 产品与权限边界

- IronPilot 是纯个人项目，按单用户运行。
- 产品定义与 Gate 不设置名义资金门槛。
- 当前仍是无杠杆 Bybit Spot；永续、保证金和做空不属于本决策。
- 任何 Mainnet 或真实资金权限都不能由本决策或 Testnet Gate 自动产生。

### 合并后唯一的长期运行验证

取消 `P3-11` 的独立 30 天本地 Paper Gate，取消 `P4-03` 的独立 72 小时 Testnet 阶段，把 `P3-11`、`P4-02B` 和 `P4-03` 合并为唯一活动 Task：

> `P4-02B — 30-day Bybit Testnet End-to-End Validation`

该 Task 在一个实际经过 30 个日历日、不可用虚拟时间压缩替代的窗口内运行完整链路：

```text
实时行情
→ AITradingPlan
→ Execution Validator
→ Bybit Testnet 下单
→ 私有订单与成交
→ 订单 / 成交 / 余额 / 受管资产对账
→ 最新事实进入 AI Decision Context
→ AI 持仓管理
```

窗口开始前冻结 Context、Prompt、Model、AITradingPlan Schema、Validator、Execution、用户最大亏损、标的、版本/hash、指标口径、停止条件和回滚方式。

### 最低运行检查

30 天窗口只强制以下四项运行检查，各完成并记录一次：

1. 服务重启；
2. 短暂网络断开；
3. 模型请求失败；
4. Emergency 验证。

本决策不要求六项正式故障注入、形式化混沌工程、100% 故障覆盖、五分钟证据间隔、无缺口运行或 `qualified` / `disqualified` 资格状态机。停机、缺失区间、异常和恢复仍须如实进入最终报告。

### 最低安全不变量

精简验证流程不得削弱以下不变量：

- 数据、时间、账户、余额、订单或状态不可信时 fail closed；
- 稳定幂等键、持久化意图、查询确认和状态机禁止重复业务效果；
- AI 方案不得超过用户最大亏损授权；
- 卖出、减仓和 Emergency 只能处理可证明受管资产；
- 服务重启必须先完成交易所对账，状态未收敛前不得恢复正常交易；
- Emergency 不依赖 AI，且完成后不自动恢复开仓；
- 正常动作、拒绝、恢复和 Emergency 必须有 correlation ID 与可定位日志。

### 统一指标

最终报告至少包含并冻结口径：

- 胜率；
- 收益率；
- 最大回撤；
- 盈亏比；
- 已闭合完整交易次数；
- Validator 拒绝率及原因码拆分；
- LLM input/output/total Token；
- 按模型和定价版本计算的 LLM 费用。

计划不预设胜率、收益率或盈亏比阈值。安全不变量是硬失败条件；绩效证据是否足以支持下一阶段由用户或授权评审者独立判断，Codex 不自行批准 Gate。

### Testnet 之后的权限

30 天 Testnet Gate 通过只是进入 `P4-04 Spot MVP / Spot Gray Eligibility Review` 的必要条件，不自动授权 Mainnet 或真实资金。

即使 `P4-04` 通过，也只允许为无杠杆 Spot 灰度独立立项。专用账户、Mainnet 凭证、用户最大亏损授权、标的、停止条件和回滚方式必须再次获得用户明确授权。任何后续权限扩展都必须基于新增证据再次审查。

## Historical Evidence Boundary

`docs/LONG_RUNNING_PAPER_SAFETY_V1.md`、相关 migration、测试和提交继续作为已完成实现的历史证据。其观测能力可以按需复用，但其中独立 30 天 Paper、五分钟间隔、六项演练和 qualification 语义不再定义活动 Task、依赖或 Gate。

## Consequences

- `P3-11` 和 `P4-03` 在活动计划中取消，`P4-02B` 成为唯一 30 天长期验证 Task。
- `P4-02B` 同时承担准备、运行、恢复检查、指标汇总和最终证据报告。
- 历史策略证据与 Testnet Protocol Smoke 仍是 `P4-02B` 前置条件。
- 运行验证更贴近真实 Testnet 协议和 AI 持仓管理链，减少重复等待和资格材料。
- 精简流程增加了遗漏长尾故障的风险；项目接受该 trade-off，并依靠 fail closed、幂等、受管资产、重启先对账、Emergency 和可定位日志限制后果。
- Testnet 绩效不能直接外推 Mainnet，因此真实资金只能在另行授权的无杠杆 Spot 灰度中验证。

## Considered Options

- **保留 30 天 Paper + 72h Testnet + qualification**：证据最严格，但验证周期重复，且超出个人可行性项目的必要工程重量。
- **只运行 72h Testnet**：反馈最快，但样本和运行周期不足以观察 AI 交易、费用、回撤与持仓管理。
- **单次 30 天 Testnet 端到端验证**：覆盖真实协议链和足够长的运行窗口，同时保留最低安全边界；本项目采用此方案。
- **Testnet 通过后直接授权 Mainnet 实盘**：无法控制 Testnet/Mainnet 差异，也绕过了真实资金的独立授权；不采用。
