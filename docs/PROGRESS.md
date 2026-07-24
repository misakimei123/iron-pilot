# IronPilot Development Progress

> 文档职责：当前实施进度与证据的唯一权威来源
>
> This file is the single source of truth for current implementation progress.
>
> Plan baseline: DEVELOPMENT_PLAN v2.2.0
>
> Plan commit: `000ccf8257f0fb5a1fa0417792710ff654ba1d56`
>
> Last updated: 2026-07-25

## Document Boundary

- 本文件不重新定义产品范围、Task 依赖、验收标准或阶段 Gate。
- 所有静态定义均引用 `docs/DEVELOPMENT_PLAN.md`。
- 本文件与开发计划冲突时，以 `docs/DEVELOPMENT_PLAN.md` 为准。
- 若发现开发计划本身需要改变，必须停止当前 Task 并申请显式计划修订，不得在本文件中改写计划。

## Current Focus

- Current phase: Phase A — Minimal Safety Kernel
- In progress: None
- Ready:
  - P1-02
- Blocked: None
- Next recommended task: P1-02

## Task Status

| Task | Status | Started | Completed | Implementation Commit | Evidence | Notes |
|---|---|---|---|---|---|---|
| `P0-01` | `DONE` | — | 2026-07-24 | `edb5f861d2377b1ee9e95da934acffd226f1f938` | `CONTEXT.md`; `docs/adr/0001-spot-first-mvp.md` 至 `0004-compositional-historical-backtesting.md` | 历史架构基线 |
| `P0-02` | `DONE` | — | 2026-07-24 | `033de133b42ce9a66b126fd8376fdd60b5b34a77` | `docs/DEVELOPMENT_PLAN.md` v2 重建及后续一致性修订 | 仅完成规划治理，未实施业务 Task |
| `P0-03` | `DONE` | 2026-07-25 | 2026-07-25 | `98f87db43c9d8c23d563ce8df43d521ea434c924` | `docs/adr/0005-bounded-ai-strategy-authority.md`; ADR-0002/0003/0004 superseded/amended 标记；`CONTEXT.md`; 语义断言与 `git diff --check` | 未修改开发计划；未批准任何阶段 Gate |
| `P1-01` | `DONE` | 2026-07-25 | 2026-07-25 | `705ca6f7b5aa4602072cc943295c15ae66bb780e` | Rust 质量门禁；空进程 smoke test；cargo-deny；Gitleaks；CI YAML 校验；零第三方 Cargo 依赖断言 | 无业务伪实现；未修改开发计划或批准 Gate |
| `P1-02` | `READY` | — | — | — | — | `P0-03` 与 `P1-01` 已完成 |
| `P1-03` | `PLANNED` | — | — | — | — | — |
| `P1-04` | `PLANNED` | — | — | — | — | — |
| `P1-05` | `PLANNED` | — | — | — | — | — |
| `P2-01` | `PLANNED` | — | — | — | — | — |
| `P2-02` | `PLANNED` | — | — | — | — | — |
| `P2-03` | `PLANNED` | — | — | — | — | — |
| `P2-04` | `PLANNED` | — | — | — | — | — |
| `P3-01` | `PLANNED` | — | — | — | — | — |
| `P3-02` | `PLANNED` | — | — | — | — | — |
| `P3-09` | `PLANNED` | — | — | — | — | — |
| `P3-03` | `PLANNED` | — | — | — | — | — |
| `P3-04` | `PLANNED` | — | — | — | — | — |
| `P3-05` | `PLANNED` | — | — | — | — | — |
| `P3-10A` | `PLANNED` | — | — | — | — | — |
| `P3-06` | `PLANNED` | — | — | — | — | — |
| `P3-07A` | `PLANNED` | — | — | — | — | — |
| `P3-08` | `PLANNED` | — | — | — | — | — |
| `P3-07B` | `PLANNED` | — | — | — | — | — |
| `P3-VS` | `PLANNED` | — | — | — | — | — |
| `P3-10B` | `PLANNED` | — | — | — | — | — |
| `P3-11` | `PLANNED` | — | — | — | — | — |
| `P4-01` | `PLANNED` | — | — | — | — | — |
| `P4-02A` | `PLANNED` | — | — | — | — | — |
| `P4-02B` | `PLANNED` | — | — | — | — | — |
| `P4-03` | `PLANNED` | — | — | — | — | — |
| `P4-04` | `PLANNED` | — | — | — | — | — |
| `D-NEWS-01` | `DEFERRED` | — | — | — | — | 不属于当前排期 |
| `P5-*` | `DEFERRED` | — | — | — | — | 永续合约；需重新授权 |
| `P6-*` | `DEFERRED` | — | — | — | — | 真实资金与扩容；需独立 Release Gate 与明确授权 |

## Active Blockers

None.

未来阻塞项必须记录：

- Task ID；
- 阻塞原因；
- 发现日期；
- 相关提交或日志；
- 解除条件；
- 是否需要修改开发计划。

## Recent Completions

### P0-01 — 历史架构基线

- 结果：建立 Spot-first 架构基线、项目上下文和 ADR-0001 至 ADR-0004。
- Commit：`edb5f861d2377b1ee9e95da934acffd226f1f938`。
- 证据：`CONTEXT.md`、`docs/DEVELOPMENT_PLAN.md` 初始基线和四份 ADR。
- 已知限制：属于历史规划基线，不代表任何业务实现、外部调用或 Gate 已完成。

### P0-02 — DEVELOPMENT_PLAN v2 权威计划重建

- 结果：建立 DEVELOPMENT_PLAN v2，并完成后续范围、依赖、权限和 Gate 一致性修订。
- Commit：`033de133b42ce9a66b126fd8376fdd60b5b34a77`。
- 证据：`docs/DEVELOPMENT_PLAN.md` v2.2.0；Plan baseline commit `000ccf8257f0fb5a1fa0417792710ff654ba1d56`。
- 已知限制：仅完成文档治理；没有开始 P0-03、P1-01 或任何业务开发 Task。

### P0-03 — ADR 与领域词汇对齐

- 结果：新增 ADR-0005，冻结有界 AI 策略权限；ADR-0002 标记为 superseded；ADR-0003/0004 与 `CONTEXT.md` 对齐 Eligibility / Event Prefilter、Strategy Intent、Deterministic Strategy Materialization 和无新闻默认链语言。
- Commit：`98f87db43c9d8c23d563ce8df43d521ea434c924`。
- 证据：`docs/adr/0005-bounded-ai-strategy-authority.md`；ADR 相对链接检查；P0-03 语义断言；`git diff --check`；`docs/DEVELOPMENT_PLAN.md` 零差异。
- 已知限制：本 Task 只完成文档与术语治理，不包含业务实现，不批准任何阶段 Gate，也不引入新闻能力。

### P1-01 — Rust 工程骨架与质量门禁

- 结果：建立 `ironpilot-domain`、`ironpilot-application`、`ironpilot-adapters` 和 `ironpilot` 四 crate 模块化单体骨架；固定 Rust 1.97.1；加入示例配置、CI、依赖治理、license/advisory/source policy 和 Git 历史秘密扫描。
- Commit：`705ca6f7b5aa4602072cc943295c15ae66bb780e`。
- 证据：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo test --workspace --all-targets --locked`（1 个 smoke test 通过）；`cargo build --workspace --all-targets --locked`；`cargo metadata` 四 crate、零第三方依赖和零 Cargo feature 断言；cargo-deny 0.19.4 的 advisories/bans/licenses/sources 全部通过；Gitleaks 8.30.1 的完整历史与工作区扫描通过；CI YAML 语法检查通过。
- 已知限制：空应用按设计不执行任何业务；配置键、领域类型、状态机和交易行为分别由后续 Task 实现；本 Task 不批准任何阶段 Gate。

## Next Action

Execute P1-02 only.

Do not start P1-03 or P1-04 until P1-02 is DONE.

以上内容是依据 `docs/DEVELOPMENT_PLAN.md` 静态依赖生成的进度建议，不改变任何 Task 依赖。

## Status Update Rules

### Task 开始时

只更新本文件：

```text
READY → IN_PROGRESS
Started → 实际开始时间
Current Focus → 当前 Task
```

### Task 完成时

只有全部验收通过后：

```text
IN_PROGRESS → DONE
Completed → 实际完成时间
Implementation Commit → 准确 SHA
Evidence → 测试或证据文档引用
```

### Task 失败或受阻时

```text
IN_PROGRESS → BLOCKED
```

同时记录阻塞证据和解除条件。

### Task 依赖满足时

根据 `docs/DEVELOPMENT_PLAN.md` 的静态依赖更新后继 Task：

```text
PLANNED → READY
```

不得自行改变依赖关系。

## Prohibited Changes

本文件不得：

- 修改 Task 范围；
- 修改直接依赖；
- 修改验收标准；
- 修改 Gate；
- 引入新能力；
- 宣布真实资金授权；
- 宣布 Testnet 写授权；
- 把未验证 Task 标为 `DONE`；
- 仅凭代码已提交就把 Task 标为 `DONE`；
- 使用本文件覆盖 `docs/DEVELOPMENT_PLAN.md`。
