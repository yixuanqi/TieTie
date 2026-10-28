# 贴贴 TieTie Windows v0.4.5

Tauri 2 / Rust / Windows WebView2。**本版重点修复：暗色主题下无边框窗口四角变直、缩放后圆角丢失。** 保留 v0.4.4 的两套主题、自由调整面板大小、快捷键自定义、系统托盘和所有便签数据。

## 修复原因

v0.4.4 将主窗口改为 `transparent:false` 来避免透出其他窗口代码，但同时将 `body` 背景铺满了整个矩形 HWND。CSS 的 `border-radius` 只裁剪内部卡片，并不会让 Windows 原生窗口的四角变透明，所以尤其在暗色模式里看起来像有尖角。

v0.4.5 **不把 WebView2 改回透明窗口**，以免重新出现桌面代码透出。改用 Windows 自带的 `CreateRoundRectRgn` + `SetWindowRgn` 为原生窗口设置与 CSS 约 21 逻辑像素一致的圆角裁剪：

- **主面板、设置面板**使用真正的原生圆角；亮色和暗色都保持实色背景；
- **拖动缩放**主面板时，在 `WindowEvent::Resized` 里同步更新裁剪区域；
- 适配 Windows 显示缩放比例：裁剪半径按窗口 DPI 换算；
- 重复尺寸通知不会重复创建裁剪区域，以降低无关重绘；
- **独立浮贴**保持原有透明窗口与可调透明度逻辑，不受影响；
- **保持** `com.tietie.desktop` 应用标识、原来的 `notes.json`、`images/`、`preferences.json` 以及数据目录，无须迁移。

**技术说明：** Win32 裁剪与暗色 CSS 两边都要生效。单纯给 `.panel` 设置 `border-radius`，不能替代原生 HWND 裁剪。这个实现不需要添加新的 Rust crate，但当前开发环境不能进行 Windows/MSVC 编译，尚需在你的电脑上验收。

## 更新方法

1. 从托盘右键退出旧版，并关闭旧项目的 `npm run dev`。
2. 可选但推荐：在旧版历史页打开「数据位置」备份（常见为 `%APPDATA%\com.tietie.desktop`）。
3. 解压新版，打开 PowerShell，切换到工程根目录，执行：

```powershell
npm install
npm run dev
```

或者运行 `npm run build` 生成 Windows Release 安装包。不要删除数据目录。

## 检查方法

- 切换 **亮色 / 暗色**：观察主窗口 **四角**是否均为圆角，而不只是内部卡片圆角。
- 将贴贴移到深色 VS Code、亮色浏览器前，窗口背景应不透字。
- 拉动 **右下角缩放手柄**把窗口放大/缩小，四角应保持一致。
- 从托盘菜单进入 **设置**，设置窗口四角也应圆润。
- 检查图片收藏、Todo、搜索、浮贴和已有数据不受影响。
- 可在新开 PowerShell 中执行 `powershell -ExecutionPolicy Bypass -File .\tests\check_windows_rounding.ps1`，检查主窗口是否持有非矩形原生裁剪区域。*这个脚本不能验证像素外观，也不代表实际渲染性能。*

## 已完成的验证及局限

以下命令对应的**前端模拟测试**、源码契约检查已经通过：

```powershell
npm run check:js
python tests/ui_smoke.py
python tests/settings_smoke.py
python tests/performance_smoke.py
python tests/rounding_contract.py
```

当前环境是 Linux Chromium，无 Rust/MSVC / Windows WebView2 运行环境。**原生 Win32 接口尚未通过实机编译和截图对比**。如果 Windows 编译报错，或边角仍有异常，请提供 PowerShell 编译错误或新版截图，方便针对实际系统继续修复。
