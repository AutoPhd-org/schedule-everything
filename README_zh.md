# Schedule Everything (晨钟暮鼓)

[![CI](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/tests.yml/badge.svg)](https://github.com/PhDeasy-org/schedule-everything/actions/workflows/tests.yml)
[![PyPI version](https://badge.fury.io/py/schedule-management.svg)](https://pypi.org/project/schedule-management)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Documentation](https://img.shields.io/badge/docs-GitHub_Pages-blue)](https://PhDeasy-org.github.io/schedule-everything/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/PhDeasy-org/schedule-everything)
[English Version](README.md)

一个面向 AI 的时间管理工具：更容易上手、本地优先，并尽量把日程建立在更健康、更科学的默认原则上，而不是单纯把时间塞满。

`rmd setup` 采用“先画像、后排程”的流程：它会先创建或更新
`profile.md`，追问关键细节，先给出摘要供你确认，最后才写入日程。
`rmd sync` 会读取当前任务，把今天的 pomodoro/potato 自动分配成具体工作事项，并在保存前先给你预览。
`rmd` 现在是主 CLI 名称；`reminder` 仍保留为兼容别名。

## 为什么要做这个

很多效率工具擅长把时间排满，却不擅长保护精力。Schedule Everything
假设真正有用的时间管理应该更容易使用、更个性化，也更尊重科学上的常识。

当你的偏好还不完整时，排程器会优先保护这些基于研究的默认原则：

- 保留足够睡眠机会，而不是长期靠压缩睡眠换工作时长
- 尽量保持规律睡眠，而不是工作日和周末大起大落
- 在一周内稳定分配活动量和运动
- 给长时间久坐工作插入短暂恢复或活动休息
- 在有弹性时，把高强度认知工作尽量放在更早、光照更好的时段

这些只是启发式原则，不构成医疗建议。真正的生活约束、医生建议、残障需求、轮班现实或照护责任都应该优先。

部分参考资料：

- Watson NF, Badr MS, Belenky G, et al. Recommended Amount of Sleep for a Healthy Adult. [AASM 共识 PDF](https://aasm.org/resources/pdf/pressroom/adult-sleep-duration-consensus.pdf)
- Sletten TL, Weaver MD, Foster RG, et al. The importance of sleep regularity. [Sleep Health, 2023](https://doi.org/10.1016/j.sleh.2023.07.016)
- World Health Organization. Physical activity recommendations for adults. [WHO 指南](https://www.who.int/initiatives/behealthy/physical-activity)
- Albulescu P, Macsinga I, Rusu A, et al. "Give me a break!" [PLOS ONE, 2022](https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0272460)
- Figueiro MG, Steverson B, Heerwagen J, et al. The impact of daytime light exposures on sleep and mood in office workers. [Sleep Health, 2017](https://doi.org/10.1016/j.sleh.2017.03.005)

## 为什么它更容易用

- `profile.md` 保存的是你的长期工作方式和约束，而不只是几个时间块名字。
- `rmd setup` 用自然语言对话来搭建日程，而不是一开始就逼你手写完整配置。
- `rmd sync` 会把 `tasks.json` 转成当天的专注块任务，并且先预览、后保存。
- 整套系统仍然是本地文件：TOML、JSON、小型 CLI，没有云端后台依赖。
- 你随时都可以手动查看和编辑生成结果，因为它们都是纯文本。

## 快速开始

### 1. 安装

使用 curl（推荐）：
```bash
curl -fsSL https://raw.githubusercontent.com/PhDeasy-org/schedule-everything/main/install.sh | bash
```

或者从本地克隆安装：
```bash
git clone https://github.com/PhDeasy-org/schedule-everything.git
cd schedule-everything
./install.sh
npm install -g @earendil-works/pi-coding-agent
```

### 2. 用 AI 创建你的日程

```bash
rmd setup
```

这个流程会构建或更新 `profile.md`，并在真正写入日程文件之前先给你一个
摘要确认。凭据和模型选择由 pi 负责（请先用 `pi` 完成认证）；可选的模型
覆盖可以写在 `~/.schedule_management/llm.toml`。首次生成会写到
`user_config_0`；之后每次确认过的修改都会在同一配置根目录下生成新的
`user_config_n` 版本，而 `tasks/` 会继续共用。

### 3. 添加任务并同步今天

```bash
rmd add "完成方案初稿" 9
rmd add "Review PR #128" 7
# 将每日紧急任务的警报延迟到明天（可选的第三个参数：天数）
rmd add "Biology homework" 9 1
rmd sync
```

如果希望每次成功执行 `rmd add ...` 或 `rmd rm ...` 后都立即显示与 `rmd ls` 相同的任务表，可以在 `[settings]` 下设置 `show_tasks_after_change = true`。

`rmd sync` 会读取 `tasks/tasks.json`，为今天的 pomodoro/potato 生成具体任务分配；如果你拒绝预览，它会带着你的反馈再排一次。

### 4. 检查结果

```bash
rmd status
rmd status -v
rmd view
rmd update
rmd switch 0
```

当今天已经有同步 overlay 时，`rmd status` 会显示块类型和具体任务标题，例如 `pomodoro: 完成方案初稿`。
`rmd update` 会重新加载提醒服务；如果配置目录本身是 git 仓库，会先拉取最新日程，否则跳过 git 步骤并直接按本地文件重载。

### 5. 可选的 macOS 桌面工具 (Daily Command Center 与 Übersicht 桌面组件)

Schedule Everything 为 macOS 用户提供了桌面集成工具：

- **Tauri 2 桌面应用 (Daily Command Center)**：和 CLI 使用同一套本地配置、任务、截止日期、习惯记录和 sync overlay 文件，提供快速添加任务/截止日期、勾选习惯、预览并确认 `rmd sync` 方案等操作。

  你可以直接从 GitHub Releases 下载预构建好的 DMG 安装包，或者自行编译：
  ```bash
  npm install
  npm run tauri:dev
  npm run tauri:build
  ```
  `npm run tauri:build` 会把 Python JSON bridge 打包成 sidecar，并把 macOS 应用包输出到 `src-tauri/target/release/bundle/`。

- **Übersicht 桌面组件 (Desktop Widget)**：可在 macOS 桌面壁纸上实时渲染显示任务列表 (`rmd-tasks.widget`)。
  - **交互式任务删除**：桌面组件上展示的任务支持直接通过 GUI 操作进行删除。任务右侧提供 `[✕]` 按钮，采用两步内联确认机制：
    - **首次点击**：按钮转换为 `[Confirm?]` 并带有红色高亮提示，进入 3 秒安全确认窗口。
    - **二次点击**：确认删除，后台自动执行 `rmd rm <id>` 并立即重新渲染桌面组件。
    - **自动重置**：若 3 秒内未点击确认，按钮将自动还原为 `[✕]`。

> [!TIP]
> **macOS “已损坏” 提示解决方法**：由于 GitHub Releases 提供的预编译 DMG 是未签名的，macOS Gatekeeper 安全机制在下载打开时会拦截并提示“已损坏，应移至废纸篓”。你可以将应用拖入 `/Applications` 后，在终端运行以下命令来解除限制：
> ```bash
> xattr -r -d com.apple.quarantine "/Applications/Schedule Everything.app"
> ```

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `rmd setup` | 用基于画像的 AI 流程构建或修改日程 |
| `rmd sync` | 为今天的 pomodoro/potato 生成任务分配，并先预览后确认 |
| `rmd status [-v]` | 查看当前状态和今天日程，包含同步后的任务标题 |
| `rmd add/ls/rm` | 管理会被 sync 使用的任务列表（`rm` 计为完成） |
| `rmd cancel` / `rmd drop` | 移除任务但不计为完成（误添加取消，或主动放弃） |
| `rmd history [n]` | 显示最近的任务活动；完成、取消、放弃会以不同样式分别展示（默认：5 条） |
| `rmd track` | 记录习惯 |
| `rmd ddl` | 管理截止日期；逾期两天及以上的条目会自动清理 |
| `rmd view` | 生成 PDF 日程可视化 |
| `rmd switch <id>` | 切换到不同的 `user_config_n` 配置版本并重载服务 |
| `rmd mode [j\|p]` | 查看或切换当前模式 (j 模式允许所有提醒，p 模式取消具体日程事件提醒) |
| `rmd settings` | 交互式 TUI 编辑 `settings.toml` 及可选的 pi 模型覆盖（方向键导航、Enter/Space 编辑、`s` 保存、`q` 退出） |

`rmd setup` 在应用日程修改前会预览文件差异。按 `Ctrl+C` 可取消；设置编辑器在有未保存的修改时会先询问是否保存。

## 手动配置和文档

README 不再展开低层手动配置流程，相关内容已移到文档站：

- [Introduction](https://PhDeasy-org.github.io/schedule-everything/docs/intro)
- [Quick Start](https://PhDeasy-org.github.io/schedule-everything/docs/quick-start)
- [Installation](https://PhDeasy-org.github.io/schedule-everything/docs/installation)
- [Configuration Overview](https://PhDeasy-org.github.io/schedule-everything/docs/configuration/overview)
- [CLI Overview](https://PhDeasy-org.github.io/schedule-everything/docs/cli/overview)

## 许可证

本项目采用 **MIT 许可证**。详见 [LICENSE](LICENSE)。
