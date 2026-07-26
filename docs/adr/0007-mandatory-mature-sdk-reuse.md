---
status: accepted
date: 2026-07-26
amends:
  - 0006-ai-dominant-trading-authority
---

# 强制复用成熟 SDK，禁止重复实现通用协议

## Context

IronPilot 已在 DEVELOPMENT_PLAN v3.1.0 建立开源优先原则，但“优先”与允许自行记录例外的表述仍留下了过大的自由裁量空间。P3-07A 因此直接使用通用 HTTP 与 JSON 库，自行实现 Telegram Bot API 方法路径、wire DTO、响应 envelope 和 long-poll 协议。该实现即使通过功能测试，也重复建设了成熟 Rust Telegram SDK 已提供的通用能力，不符合用户要求。

用户在 2026-07-26 明确要求：优先使用成熟流行的开源 SDK，绝对禁止自行封装一套已有 SDK 覆盖的重逻辑，并要求使用开源流行库重构 Telegram。

## Decision

对于 Telegram、AI Provider、交易所和其他外部标准协议：

1. 只要存在满足当前功能、安全、许可证与资源边界的成熟、流行、持续维护 SDK，就必须采用该 SDK。
2. 项目代码不得重新实现 SDK 已覆盖的协议方法、endpoint 路径、请求/响应 DTO、响应 envelope、轮询、分页、重试或错误协议。
3. 项目可以保留薄的领域适配层，但该层只能映射 IronPilot 领域合同、权限、预算、证据和安全上限，不得复制通用协议实现。
4. 必须记录候选库的维护活跃度、社区采用、许可证、资源重量、默认重试/遥测与边界适配，并锁定版本、执行供应链门禁。
5. 若所有成熟候选均无法满足必要边界，必须先取得用户明确授权并独立修订 DEVELOPMENT_PLAN；Codex 不得仅凭实施判断或 `PROGRESS.md` 说明批准自研。

P3-07A 采用 `teloxide-core` 承载 Telegram Bot API client、`getUpdates`、`sendMessage`、Telegram 类型和响应解析。IronPilot 只保留 chat allowlist、只读命令映射、SQLite 查询、领域文本、批次上限及 cursor/audit 边界。

## Consequences

- 删除 P3-07A 自建的 Bot API endpoint、wire DTO、envelope 与通用 HTTP response 解码。
- Telegram contract tests 面向 SDK 发出的真实请求和 SDK 类型解码，不再测试项目自建协议模型。
- SDK 默认行为必须显式审计；不得启用隐藏重试、遥测或不可审计请求改写。
- 增加依赖是可接受成本，但只引入满足 Task 所需的最小 SDK crate 与 feature。
- 本 ADR 不改变 Task 表、依赖图、阶段顺序、交易权限或 Gate 批准权。
