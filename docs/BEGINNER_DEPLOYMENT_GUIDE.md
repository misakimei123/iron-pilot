# IronPilot 新手上线准备与配置指南

> 面向第一次接触本项目、Rust、API Key 和自动交易的使用者。
>
> 最后核对日期：2026-07-28
>
> 本文只说明当前仓库真实具备的能力，不代表批准任何 Bybit Testnet 写操作、Mainnet
> 实盘交易或阶段 Gate。

## 1. 先读结论：现在能不能“一键上线”

目前**不能把 IronPilot 当成完整的自动交易程序直接上线**。

当前 `ironpilot` 主程序只会：

1. 读取 YAML 配置；
2. 检查环境、权限、版本和资源限制；
3. 配置正确时正常退出，配置错误时拒绝启动。

它目前不会在后台持续运行，也不会自动连接 DeepSeek、Telegram 或 Bybit 下单。仓库中已实现
AI、行情、Paper、Telegram、Bybit 私有流和执行等模块，但这些模块还没有全部组装进一个可长期
运行的正式入口。

当前可直接操作的入口如下：

| 操作 | 当前状态 | 是否使用真实资金 | 新手建议 |
|---|---|---:|---|
| 校验启动配置 | 可用 | 否 | 可以操作 |
| 编译 `ironpilot.exe` | 可用，但程序仍只校验配置 | 否 | 可以操作 |
| Bybit Testnet protocol smoke | 可用，属于受限测试工具 | 否，只使用测试币 | 理解风险并获得明确授权后再操作 |
| 30 天 Bybit Testnet 完整自动交易 | `READY`，但可运行入口和冻结配置尚未交付 | 否，只应使用测试币 | 现在不要启动 |
| Bybit Mainnet 实盘 | 未授权、未交付 | 是 | 禁止操作 |

项目当前进度以 [`PROGRESS.md`](PROGRESS.md) 为准。静态范围和 Gate 以
[`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md) 为准。

## 2. 新手需要知道的几个词

- **PowerShell**：Windows 自带的命令行工具。本文中的命令都应在 PowerShell 中执行。
- **项目根目录**：包含 `Cargo.toml`、`README.md`、`crates/` 和 `docs/` 的目录。当前示例为
  `D:\workspace\iron-pilot`。
- **API Key / API Secret / Bot Token**：让程序代表你的账号访问外部服务的密码。泄漏后，
  别人可能消耗你的 AI 余额、控制 Bot，甚至使用交易权限。
- **环境变量**：只交给运行中程序的配置值。密钥应通过环境变量或专用秘密管理工具注入，
  不应写进 Git、YAML、代码、截图或聊天消息。
- **Paper**：使用真实行情或模拟数据进行纸面交易，不向交易所提交订单。
- **Testnet**：交易所提供的测试环境，使用测试币，不是现实资金。
- **Mainnet / Live**：真实交易环境，会影响现实资金。
- **DPAPI**：Windows 提供的本机加密能力。仓库中的 Bybit Testnet 凭证脚本使用 DPAPI，
  加密文件通常只能由创建它的同一 Windows 用户在同一台机器上解密。

## 3. 到底要准备哪些账号和密钥

### 3.1 最小准备清单

如果你现在只想验证项目能否编译、配置能否通过：

- 不需要任何 API Key；
- 不需要 Bybit 账号；
- 不需要 Telegram Bot。

如果未来要运行完整的 30 天 Bybit Testnet 验证，预计至少需要：

| 服务 | 需要的内容 | 项目读取名称 | 现在是否必须申请 |
|---|---|---|---|
| DeepSeek | API Key 和可用 API 余额 | `IRONPILOT_DEEPSEEK_API_KEY` | 建议准备 |
| Bybit Testnet | Testnet API Key + API Secret | smoke 工具使用专用 DPAPI 文件 | 建议准备 |
| Telegram | Bot Token | `IRONPILOT_TELEGRAM_BOT_TOKEN` | 可选 |
| Bybit Mainnet | Mainnet API Key + API Secret | 当前没有获准的正式配置 | **不要申请给本项目使用** |

### 3.2 不属于密钥、但启动时必须提供的三个值

下面三个环境变量不是秘密：

| 环境变量 | 作用 | 示例 |
|---|---|---|
| `IRONPILOT_CONFIG_PATH` | 告诉程序 YAML 在哪里 | `config/ironpilot.example.yaml` |
| `IRONPILOT_ENVIRONMENT` | 声明本次启动环境 | `development` |
| `IRONPILOT_ENVIRONMENT_FINGERPRINT` | 防止拿错配置 | `development-paper-local` |

它们必须与 YAML 中的 `environment.name` 和 `environment.fingerprint` 完全一致。

## 4. 准备 Windows 电脑

### 4.1 建议环境

- Windows 10/11 64 位；
- 至少 2 个可用 CPU 核心；
- 运行配置为 2 GiB 内存上限，编译时还需要额外可用内存和磁盘；
- 可访问 GitHub、Rust crate 源、DeepSeek、Bybit Testnet 和 Telegram 的网络；
- 使用普通 Windows 用户运行，不要为了省事长期使用管理员 PowerShell。

### 4.2 安装 Git

从 [Git for Windows 官网](https://git-scm.com/download/win) 安装。安装后重新打开
PowerShell，检查：

```powershell
git --version
```

### 4.3 安装 Rust 和 C++ Build Tools

1. 打开 [Rust 官方安装页](https://www.rust-lang.org/tools/install)；
2. 下载并运行 64 位 `rustup-init.exe`；
3. 如果安装程序提示缺少 Visual Studio C++ Build Tools，按提示安装；
4. 安装完成后关闭并重新打开 PowerShell。

项目固定使用 Rust `1.97.1`，并要求 `rustfmt` 和 `clippy`。进入项目后，Rust 会根据
`rust-toolchain.toml` 选择对应工具链。

检查安装：

```powershell
rustup --version
rustc --version
cargo --version
```

## 5. 获取项目并进行第一次安全启动

### 5.1 下载项目

如果电脑上还没有项目：

```powershell
Set-Location D:\workspace
git clone https://github.com/misakimei123/iron-pilot.git
Set-Location D:\workspace\iron-pilot
```

如果项目已经存在：

```powershell
Set-Location D:\workspace\iron-pilot
git status --short --branch
git pull --ff-only
```

`git pull` 前应先通过 `git status` 确认自己没有尚未保存的本地修改。如果输出中存在文件变更，
先停止，不要用覆盖或强制命令处理。

### 5.2 第一次运行配置校验

在项目根目录执行：

```powershell
$env:IRONPILOT_CONFIG_PATH = "config/ironpilot.example.yaml"
$env:IRONPILOT_ENVIRONMENT = "development"
$env:IRONPILOT_ENVIRONMENT_FINGERPRINT = "development-paper-local"
cargo run --locked
$LASTEXITCODE
```

预期结果：

- 第一次运行会下载并编译依赖，耗时可能较长；
- 配置正确时，程序不输出交易信息并立即退出；
- `$LASTEXITCODE` 应为 `0`；
- 这只证明“配置可被当前程序接受”，不代表交易服务已经启动。

验证结束后清理本次 PowerShell 会话中的变量：

```powershell
Remove-Item Env:IRONPILOT_CONFIG_PATH -ErrorAction SilentlyContinue
Remove-Item Env:IRONPILOT_ENVIRONMENT -ErrorAction SilentlyContinue
Remove-Item Env:IRONPILOT_ENVIRONMENT_FINGERPRINT -ErrorAction SilentlyContinue
```

### 5.3 只编译 release 程序

```powershell
cargo build --release --locked -p ironpilot
```

生成文件位于：

```text
target\release\ironpilot.exe
```

必须再次强调：当前这个 exe 仍然只做配置校验，不是完整常驻交易服务。仓库当前也没有
Dockerfile、Windows Service 安装脚本或正式 release 部署包。

## 6. 理解和修改 YAML 配置

示例配置位于 [`../config/ironpilot.example.yaml`](../config/ironpilot.example.yaml)。
密钥不能写入这个文件。

### 6.1 推荐的本地配置位置

如果以后确实需要修改配置，建议把副本放在 Git 仓库外：

```powershell
$configDirectory = Join-Path $env:LOCALAPPDATA "IronPilot"
New-Item -ItemType Directory -Path $configDirectory -Force
Copy-Item .\config\ironpilot.example.yaml (Join-Path $configDirectory "ironpilot.yaml")
```

然后用文本编辑器打开：

```powershell
notepad (Join-Path $configDirectory "ironpilot.yaml")
```

启动时改用：

```powershell
$env:IRONPILOT_CONFIG_PATH = Join-Path $env:LOCALAPPDATA "IronPilot\ironpilot.yaml"
```

### 6.2 每组配置是什么意思

| YAML 区域 | 新手解释 |
|---|---|
| `schema_version` | 配置格式版本，不要自行修改 |
| `environment` | 环境名和防拿错配置的指纹 |
| `permissions` | 允许哪一种执行模式，以及是否允许 AI 交易方案 |
| `versions` | 行情特征、AI Context 和 AI Plan 的合同版本 |
| `instruments` | 允许处理的现货交易标的 |
| `runtime` | CPU、内存、标的数量和活动计划数量上限 |
| `llm` | AI 并发、每日调用、Token 和费用上限 |
| `market` | 每个周期保留多少根 K 线 |
| `storage` | SQLite 连接和写并发限制 |
| `queues` | 内部事件队列容量 |

### 6.3 当前不能随意改的边界

- `environment.name` 当前只接受 `development` 或 `paper`；
- `permissions.execution_mode` 虽然类型中存在 `testnet` 和 `live`，但当前启动校验明确拒绝
  高于 `paper` 的模式；
- 只允许 1–3 个 Bybit Spot 标的；
- 当前示例标的是 `bybit:spot:BTCUSDT`；
- 资源值可以调低到合法正数，但不能超过代码中的固定上限；
- `versions` 必须与当前代码支持的版本完全一致；
- 不认识的 YAML 字段会被拒绝；
- 不认识的 `IRONPILOT_*` 环境变量也可能导致启动被拒绝。

因此，不要为了“开启实盘”把 `execution_mode` 改成 `live`，也不要自行添加
`enable_live: true`、主网地址或 API Key 字段。

## 7. 申请 DeepSeek API Key

### 7.1 申请步骤

1. 打开 [DeepSeek 开放平台](https://platform.deepseek.com/)；
2. 注册或登录账号；
3. 进入 API Keys 页面；
4. 创建一把只给 IronPilot 使用的新 Key；
5. 立即复制并保存在密码管理器中；
6. 确认 API 账户有可用余额，并在正式测试前查看当前模型和价格。

DeepSeek 的[官方快速入门](https://api-docs.deepseek.com/)会链接到当前 API Key 申请入口，
并列出当前可用模型。项目代码当前支持 `deepseek-v4-flash` 和 `deepseek-v4-pro`。

### 7.2 在项目中的配置名称

项目读取：

```text
IRONPILOT_DEEPSEEK_API_KEY
```

不要把 Key：

- 写进 `config/ironpilot.example.yaml`；
- 写进 `.ps1`、`.rs`、Markdown 或 Git commit；
- 通过截图、聊天或日志发送；
- 使用 `setx` 长期明文保存。

当前主程序还没有启动 DeepSeek provider，所以**现在只需安全保存 Key，不需要把它设置到
系统环境变量中**。等完整 Testnet runtime 入口交付后，应由运行脚本或秘密管理工具只注入到
目标进程。

### 7.3 费用边界

示例 YAML 中的：

```yaml
llm:
  max_concurrency: 1
  daily_call_limit: 40
  daily_token_limit: 200000
  daily_cost_limit_usd: "2.00"
```

是 IronPilot 的本地预算上限，不等于 DeepSeek 账户余额，也不能代替 DeepSeek 平台的账单和
用量检查。模型价格会变化，开始长时间测试前必须重新核对官方价格。

## 8. 申请 Bybit Testnet API Key

### 8.1 必须使用独立的 Testnet 账号

1. 打开 [Bybit Testnet](https://testnet.bybit.com/)；
2. 在 PC 浏览器注册 Testnet 账号；
3. 完成安全验证；
4. 进入 `Assets → Assets Overview → Request Test Coins` 领取测试币；
5. 确认资产位于 Spot 账户。

Bybit 官方说明见
[注册 Testnet 和领取测试币](https://www.bybit.com/en/help-center/article/How-to-Request-Test-Coins-on-Testnet)。
Testnet 与 Mainnet 是不同环境。**不要向 Testnet 地址充值真实资产**，官方明确提示这样可能造成
永久损失。

### 8.2 创建 API Key

1. 登录 Testnet；
2. 打开 [Testnet API Management](https://testnet.bybit.com/app/user/api-management)；
3. 点击 `Create New Key`；
4. 创建 system-generated API Key；
5. 设置为可读写，但只授予 **Spot Trade** 所需权限；
6. 不授予 Withdraw、Wallet Transfer、Contract、Derivatives、Options、Earn、P2P 等权限；
7. 完成 2FA；
8. 立即保存 `API Key` 和只显示一次的 `API Secret`。

Bybit 官方创建流程见
[How to Create Your API Key](https://www.bybit.com/en/help-center/article/How-to-create-your-API-key)；
官方权限字段说明见
[Get API Key Information](https://bybit-exchange.github.io/docs/v5/user/apikey-info)。

如果使用 IP 白名单：

- 填运行机器或代理出口的**公网 IP**；
- 不要把 `127.0.0.1` 当成 Bybit 能看到的公网 IP；
- 公网 IP 变化后，旧白名单可能导致认证失败。

### 8.3 用仓库脚本加密保存

仓库只为 Bybit Testnet smoke 提供了 DPAPI 凭证脚本。请在项目根目录执行：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\set-bybit-testnet-credential.ps1
```

脚本会分别提示输入 API Key 和 API Secret，输入内容不会回显。加密文件保存到：

```text
%USERPROFILE%\.ironpilot\bybit-testnet-credential.clixml
```

这个文件仍然属于敏感文件：

- 不要复制进项目；
- 不要提交到 Git；
- 不要上传网盘或发送给别人；
- 不要把它当成跨机器备份；
- Key 泄漏或机器丢失时，应立即在 Bybit Testnet 删除旧 Key 并创建新 Key。

### 8.4 不要把 smoke 当成正式上线

Testnet smoke 会在 `BTCUSDT` 上执行受限的下单、查询、撤单、买入、Emergency 卖出和重启对账，
单笔参考金额上限为 10 USDT 测试币。它会产生真实的 Testnet 写操作，因此仍需明确授权。

运行说明和安全边界见
[`BYBIT_TESTNET_PROTOCOL_SMOKE_V1.md`](BYBIT_TESTNET_PROTOCOL_SMOKE_V1.md)。

当前 Windows wrapper 按项目已验证的**本机回环代理**路径编写：Windows 系统代理必须指向
`127.0.0.1`、`localhost` 或 `::1`。如果你不使用本机代理，或不理解代理出口、TLS 和 IP
白名单，不要自行修改 endpoint、关闭 TLS 校验或改用 Mainnet；应先停止并让开发者补充适合
当前网络的受审查启动方式。

## 9. 创建 Telegram Bot（可选）

### 9.1 创建 Bot Token

1. 在 Telegram 中打开官方 [@BotFather](https://t.me/BotFather)；
2. 发送 `/newbot`；
3. 按提示输入 Bot 显示名称；
4. 设置以 `bot` 结尾的唯一用户名；
5. 保存 BotFather 返回的 Token；
6. 打开新 Bot 的聊天窗口并发送 `/start`。

官方教程见
[From BotFather to Hello World](https://core.telegram.org/bots/tutorial)。

项目读取：

```text
IRONPILOT_TELEGRAM_BOT_TOKEN
```

Token 等同于 Bot 密码。泄漏后应立即通过 BotFather 撤销并生成新 Token。

### 9.2 当前配置边界

Telegram adapter 还要求 chat allowlist；Emergency 还要求独立的 user allowlist 和二次确认。
这些 allowlist 当前没有接入主程序的 YAML 或命令行入口。因此：

- 只设置 Bot Token 不会让当前 `ironpilot.exe` 启动 Bot；
- Bot 没有回复不代表 Token 错误；
- 不要为了获取 chat ID 把 Token 粘贴到不可信网站或第三方 Bot；
- 等正式 runtime 提供明确的 allowlist 配置和启动入口后再完成接入。

## 10. 密钥安全规则

必须遵守：

1. Testnet 和 Mainnet 使用完全不同的账号与 Key；
2. 每个项目单独创建 Key，不复用个人其他工具的 Key；
3. 只给最小权限；
4. 永远不授予提现权限；
5. 密钥不写入 YAML、Git、代码、文档和日志；
6. 不把密钥作为命令行参数，因为命令行可能被历史记录或进程列表看到；
7. 不在聊天中发送完整 Key，排障时最多提供打码后的末 4 位；
8. 定期检查 DeepSeek 用量和 Bybit API Key；
9. 不再使用时立即删除或撤销；
10. 怀疑泄漏时先撤销，不要等待确认。

文档、Issue、截图和日志中不能出现任何真实 Key、Secret 或 Token；需要说明配置格式时，只写
环境变量名，不写看起来像真实凭证的示例值。

## 11. 常见问题

### 11.1 `required environment variable ... is missing`

含义：三个启动变量中至少一个没有设置。

处理：重新执行第 5.2 节的三条 `$env:...` 命令。

### 11.2 `configured environment ... does not match expected ...`

含义：PowerShell 环境变量与 YAML 中的环境名不一致。

处理：让 `IRONPILOT_ENVIRONMENT` 与 YAML 的 `environment.name` 完全一致。

### 11.3 `configured environment fingerprint does not match`

含义：环境指纹不一致，程序为防止拿错配置而拒绝启动。

处理：让 `IRONPILOT_ENVIRONMENT_FINGERPRINT` 与 YAML 完全一致，不要通过删除检查绕过。

### 11.4 `execution mode ... is not authorized`

含义：尝试把配置改成 `testnet` 或 `live`，超过当前主程序授权。

处理：恢复为 `paper`。不要修改代码绕过。

### 11.5 `unknown IronPilot environment variable ...`

含义：当前 PowerShell 中残留了主程序不认识的 `IRONPILOT_*` 变量。

处理：先查看变量名，不要输出其值：

```powershell
Get-ChildItem Env: | Where-Object Name -Like "IRONPILOT_*" | Select-Object Name
```

确认后只删除目标会话中的残留变量，再重试。Bybit smoke wrapper 会在 `finally` 中清理自己的
临时变量。

### 11.6 Rust 编译提示找不到 linker、`link.exe` 或 Windows SDK

含义：通常是 Visual Studio C++ Build Tools 未正确安装。

处理：重新打开 Visual Studio Installer，安装 C++ desktop build tools 和 Windows SDK，
然后重启 PowerShell。

### 11.7 Bybit 返回 API Key 或权限错误

逐项检查：

- 是否登录并创建了 **Testnet** Key，而不是 Mainnet Key；
- 是否启用了 Spot Trade；
- 是否误设了过期或错误的 IP 白名单；
- Windows 系统时间是否准确；
- Key 是否已删除、过期或被重新生成；
- 运行流量是否从预期的代理公网出口发出。

不要通过开启提现、合约或全部权限来“试试看”。

### 11.8 DeepSeek 没有被调用，Telegram Bot 没有回复

当前主程序没有组装这两个长期运行入口，这是项目现状，不是通过多设置几个环境变量就能解决的
配置问题。

## 12. 未来真正启动 30 天 Testnet 前的验收清单

只有以下事项全部完成后，才适合编写最终“一键启动”步骤：

- [ ] 已交付并审查完整 Testnet runtime 入口；
- [ ] 实时行情 → DeepSeek → Validator → Bybit Testnet → 私有成交 → 对账 → AI 持仓管理
  已串联；
- [ ] 冻结 Context、Prompt、Model、AI Plan Schema、Validator、Execution 和版本/hash；
- [ ] 冻结 Testnet 标的、最大亏损授权、指标口径、停止条件和回滚方式；
- [ ] DeepSeek Key、Bybit Testnet Key 由运行环境安全注入；
- [ ] Telegram allowlist 和 Emergency operator allowlist 有明确配置入口；
- [ ] 数据库路径、备份、日志和磁盘空间方案明确；
- [ ] 服务重启必须先对账再恢复；
- [ ] 已设计网络中断、模型失败和 Emergency 验证；
- [ ] 用户单独授权本次 30 天 Testnet 写窗口；
- [ ] 没有 Mainnet Key、真实资金、杠杆、永续或提现权限。

30 天 Testnet 完成后，还必须由用户或授权评审者独立检查证据。通过 Testnet 也只产生后续
无杠杆 Spot 灰度的审查资格，**不会自动授权 Mainnet 实盘**。

## 13. 新手最安全的下一步

现在建议只做以下三件事：

1. 按第 4 节安装 Rust；
2. 按第 5.2 节完成一次无密钥配置校验；
3. 分别申请并安全保存 DeepSeek Key 和 Bybit Testnet Key，但不要启动 30 天写窗口。

如果第 5.2 节成功，你已经完成当前主程序真实支持的“本地上线前检查”。下一阶段应由开发者先
交付 P4-02B 的冻结配置、完整 runtime 入口、停止条件和回滚方式，再补充本文中的最终启动与
运维步骤。
