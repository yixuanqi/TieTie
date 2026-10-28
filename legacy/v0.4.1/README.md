# 贴贴 · TieTie Windows v0.4.1（Tauri 原生桌面工程）

> 随手存下，随时找回，想贴就贴。

这是一个 **真正的 Windows 常驻桌面应用项目**，不是网页原型。Tauri 2 以 Rust 实现系统托盘、Windows 全局快捷键、系统剪贴板、数据读写和独立的 Windows 浮贴窗口。界面由系统 WebView2 渲染，仅使用原生 HTML/CSS/JavaScript，**没有 Qt、Electron、React、Vue 或 Vite**。

**交付状态**：本仓库包含可构建的源码、单元测试及 Windows CI 配置。已完成 JavaScript 语法检查和浏览器模拟交互测试；由于提供方环境缺少 Rust/MSVC/Windows，**没有完成 Windows 下的 `cargo build` 或本机 EXE 运行验收**。Windows 兼容性需在真实 Windows 环境按本文件的验收清单复测；不可将源码误认为已发布的最终稳定版。

## v0.4.1 · 剪贴板图片兼容修复（2026-10-28）

此版本修复 v0.4 在 PixPin 等软件复制图片后显示“剪贴板里没有可读取的文字或图片”的问题。根因是 `arboard::get_image()` 不支持部分合法的 Windows `CF_DIB`/`CF_DIBV5` 变体（例如 PixPin 使用的 32 位 BI_BITFIELDS）。**请使用本工程替换 v0.4 源码，并重新运行 `npm run dev`。**

改动如下：

- 在主面板按 `Ctrl+V` 时，如果 WebView2 的粘贴事件直接提供 `image/*` Blob，就由前端读取并交给 Rust 解码，绕开 `arboard` 的 Windows 图像解析限制；文本粘贴仍由系统原生编辑框处理。
- 点击粘贴按钮、`Ctrl+Alt+V` 和托盘的“收藏剪贴板”，仍优先走快速的 `arboard` 通道；当 Windows 检测到 `CF_BITMAP` / `CF_DIB` / `CF_DIBV5` / `CF_HDROP` 而快速解码失败时，转用 Windows PowerShell 5.1 的 `System.Windows.Forms.Clipboard.GetImage()` / GDI+ 兜底。
- 支持 Windows 资源管理器复制单个常见图片文件（PNG/JPG/BMP/GIF/TIFF）后快捷收藏。对于复制多个文件、私有格式或 WebP 文件路径，尚未全面适配。
- 图片数据通过 Rust 验证格式和尺寸后以真正的 PNG 保存在既有数据目录中；图片/文字优先级保持图片优先；补充了更明确的错误提示。
- 兼容读取在线程池中进行，避免启动 PowerShell 时阻塞窗口渲染。
- 原始记录数据路径与应用标识 `com.tietie.desktop` **没有修改**。升级不会主动清空已有便签。如数据重要，升级前先备份 `%APPDATA%\com.tietie.desktop` 目录。

### 在你电脑上验收

1. 关闭旧的贴贴进程（托盘右键 → 退出），解压 v0.4.1 源码，进入新工程目录，执行 `npm install`、`npm run dev`。
2. PixPin 截取一张不含隐私的测试图，确认已复制到剪贴板，然后打开贴贴输入框按 `Ctrl+V`：应该出现图片预览，提示“图片已长期保存到本地”。
3. 再截图一次，不打开主面板，按 `Ctrl+Alt+V`：应自动在历史里新增一张图片。
4. 分别测试 Windows 截图工具、浏览器右键“复制图片”、微信复制图片和普通文字。最后退出并重启贴贴，确认图片仍可找回。
5. 如果仍失败，运行 `powershell.exe -Sta -File .\docs\clipboard_diagnostics.ps1`，把输出的格式列表和错误文字发过来（诊断脚本**不导出图片内容或剪贴板文字**）。

**验证边界**：Node 的 JavaScript 语法检查、浏览器粘贴 Blob 的模拟测试及旧功能冒烟测试已通过；当前环境无 Rust/MSVC/Windows，无法确认 Windows 下的构建和真实 PixPin 粘贴已通过。这次属于有针对性的修复候选，仍需你的 Windows 实机反馈。

---

## 1. Windows 本地运行

1. 安装 [Node.js LTS](https://nodejs.org/)、[Rust（rustup）](https://rustup.rs/) 以及 **Visual Studio 2022 Build Tools / Desktop development with C++（MSVC + Windows SDK）**。
2. Windows 10/11 需要 Microsoft Edge WebView2 Runtime；Windows 11 通常已安装。如果缺少，先安装它。
3. 解压本工程，进入 `TieTie_Windows_v0.4` 目录（PowerShell）：

```powershell
npm install
npm run check:js
npm run dev
```

第一次运行会编译 Rust 依赖。成功后它是一个独立的 Windows 桌面进程：首次显示快捷面板，后续可关闭到系统托盘。

生成 NSIS Windows 安装包：

```powershell
npm run build
```

安装程序预期路径：`src-tauri\target\release\bundle\nsis\*.exe`。

### 没有 Windows 构建环境？

仓库内含 `.github/workflows/windows-build.yml`：将项目推到你自己的 GitHub 仓库后，从 **Actions → Build TieTie for Windows → Run workflow** 触发，成功构建后在运行记录的 Artifacts 中获取 Windows 安装包。上传前请检查 GitHub 工作流的运行权限及计费政策；当前交付不包含已经执行成功的云端构建。

## 2. 使用方法

| 动作 | 结果 |
|---|---|
| `Ctrl + Alt + N` | 全局显示 / 隐藏快捷收集面板 |
| `Ctrl + Alt + V` | 不打开面板，把系统剪贴板中的**图片优先、否则文字**存到本地 |
| 在面板按 `Ctrl + V` | 粘贴来自 PixPin、微信、浏览器、Snipping Tool 等应用的图片或文字 |
| 点击面板「▣」 | 手动读取剪贴板（解决部分 WebView 粘贴事件格式不兼容的问题） |
| `Esc` / 左键双击面板非编辑区 | 自动收好当前文字/清单，隐藏面板 |
| 点击「找回来」 | 根据关键词、类型、今天 / 近 7 天 / 本月 / 自定义日期搜索 |
| 点击记录左侧 | 继续编辑文字、图片说明或待办 |
| 记录右侧「♧」 | 创建或恢复**独立的 Windows 浮贴窗口** |
| 浮贴左键双击图片、正文非编辑区域或标题栏 | 隐藏当前浮贴（**不会删除**） |
| 浮贴拖动标题栏 / 拖动窗口边缘 | 更改位置 / 大小（自动保存） |
| 浮贴悬停操作栏 | 置顶 / 取消置顶、透明度、隐藏 |
| 系统托盘左键 | 显示 / 隐藏面板；右键菜单可打开、收藏或退出 |

**Todo**：在「随手记」里切到 ☑，输入任务并回车；勾选后有短时动画与“NASA 来借执行力”等离线幽默鼓励。默认无音效、不接大模型 API。

如果热键已被别的程序占用，会出现提示；仍可通过托盘菜单使用。首版没有自定义热键设置。

## 3. 用户数据目前存在哪里

临时采用**可迁移的本地 JSON + PNG 目录**，这是 v0.4 系列为支持真正持久化做出的**阶段性实现选择**，不是对后续数据方案的最终结论。

默认保存在 Tauri 应用数据目录（通常类似）：

```
%APPDATA%\com.tietie.desktop\
    notes.json          # 所有元数据/文字/任务/时间/浮贴设置
    notes.json.bak      # 最近一次写入前的 JSON 备份
    images\
        <uuid>.png      # 独立保存的真实 PNG，非缓存/外部临时引用
```

主面板「找回来」→「数据位置」可查看真实路径。

实现细节：
- 每条卡片都有 UUID、创建/修改时间；Todo 单项记录完成时间；浮贴记住位置、尺寸、透明度、置顶和可见状态。
- 本地数据由 Rust 管理，避免把图片数据长久写在浏览器 localStorage 中。
- 写入采用临时文件 + rename，并在覆盖前保存 `.bak`；若 JSON 解析失败会报错而不是清空数据。
- 删除是**软删除**，提供短暂撤销入口。现阶段不会立即清理已删除图片文件。
- 无云同步、无默认复制监听、无需账户、无需 API Key。
- **尚未实现完整回收站管理、自动备份策略、导出/导入、加密、OCR、跨设备同步及大规模搜索索引**。这些应在下一轮数据模型讨论后设计，不建议现阶段存入唯一不可替代的敏感资料。

## 4. 项目结构

```
web/                      # Vanilla HTML/CSS/JS，桌面窗口 UI
  index.html, app.js       # 快捷捕捉、搜索、Todo
  float.html, float.js     # 每条浮贴的单独 Windows 窗口
  styles.css              # 米白、雾蓝、轻手绘、半透明
src-tauri/
  src/lib.rs               # 热键、托盘、剪贴板、窗口、IPC
  src/storage.rs           # JSON + PNG 持久化与测试
  capabilities/default.json
  tauri.conf.json
.github/workflows/windows-build.yml
  
docs/                    # v0.3 基线设计和 v0.4 界面测试截图
 tests/ui_smoke.py         # 浏览器 mock Tauri API 的交互冒烟测试
```

### 手动验收清单（Windows 实机必做）

- [ ] 首次启动能显示主面板；关闭后进程及托盘仍在。
- [ ] `Ctrl + Alt + N` 在 VS Code / 微信前台状态下仍可切换窗口。
- [ ] PixPin 图片、系统截图、浏览器复制图片、微信图片、普通文本都可以复制并按 `Ctrl + Alt + V` 收藏；不能读取时有错误提示。
- [ ] 图片保存后清空 Windows 剪贴板、退出 PixPin、重启贴贴，图片依然存在。
- [ ] 粘贴文字、Todo 完成状态、按关键词 + 日期组合搜索；退出/重启后仍可找到。
- [ ] 同时钉出多个独立浮贴；拖动、调整透明度及尺寸，再重启仍复原。
- [ ] 浮贴双击能隐藏，文字输入框双击能选词，隐藏不删内容。
- [ ] Windows 100%/150%/200% DPI 与双显示器下窗口坐标正确，未离屏。
- [ ] 快捷键冲突、剪贴板为空、图片大于限制、磁盘满、JSON 文件损坏等异常有可理解的反馈。

## 5. 当前范围与已知缺口

**已实现源码**：Windows 原生独立窗口、系统托盘、热键、主动剪贴板保存、文本/图片/Todo、自动保存、关键词与时间筛选、浮贴恢复/移动/尺寸/置顶/透明度/双击隐藏、Todo 鼓励文字。

**待 Windows 验证或进一步打磨**：部分应用的私有图片剪贴板格式、特殊双显示器/DPI 场景、全局快速收藏成功后独立的短暂桌面提示（当前提示显示在主面板）、动画/磨砂在不同 Windows 版本上的效果、性能与图片缩略图缓存。

**未做**：开机自启、OCR、自动监听/敏感内容过滤、标签、导出、云同步、加密、资源占用基准测试、Windows 代码签名。

*基线设计文档：`docs/产品设计_v0.3_基线.md`。本文内容为开发实现说明，而非最终持久化架构决策。*
