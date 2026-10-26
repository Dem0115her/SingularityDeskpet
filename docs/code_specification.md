# Singularity 代码说明（Code Specification）

> **项目**：奇点 / Singularity — Windows 黑洞桌宠
> **版本**：1.5.0
> **Git 基线**：`release: singularity 1.5.0` 单一提交；旧 1.4.0 开发基线及旧文档已在发布前移除
> **文档生成**：2026-10-26
> **读者**：接手开发的工程师、维护者、验收人员

本文描述**当前源码结构、运行机制、数据模型、各子系统的实现细节和扩展方式**。用户向介绍见 `README.md`。

> 1.5.0 变更：设置面板与悬停 HUD 重做为紧凑玻璃卡片；HUD 增加待办摘要；抽出共享 `src/ui.rs`（egui 画布、主题、组件与动画辅助）；删除测试图片/脚本、`.wgsl.bak`、重复文档和确认无用的僵尸代码；文档收敛为 `README.md` + 本说明。

---

## 目录

1. 快速开始与版本信息
2. 项目结构
3. 总体架构
4. 运行时生命周期与帧调度
5. Win32 窗口与消息路由
6. 配置数据模型与持久化
7. 渲染子系统
8. 设置面板（panel.rs + ui.rs）
9. 悬停系统状态 HUD（stats.rs + sysmon.rs + ui.rs）
10. 趣味互动（fun.rs + desktop_icons.rs）
11. 提醒、专注、待办、便签、媒体
12. Win32 工具层（win.rs）
13. 调试、测试与可观测性
14. 已知限制与清理记录
15. 扩展指南
16. 交接检查清单

---

## 1. 快速开始与版本信息

### 1.1 构建

如果 Rust 与 MSVC 工具链已在 PATH 中：

```powershell
cargo build --release
```

否则可先进入 Visual Studio 开发者环境（项目机上可用）：

```powershell
cmd.exe /c 'call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && set "PATH=C:\Users\20922\.cargo\bin;%PATH%" && cargo build --release'
```

开发构建：

```powershell
cargo build
cargo run
```

测试与静态检查：

```powershell
cargo test
cargo clippy --all-targets
```

便携包：

```powershell
powershell -ExecutionPolicy Bypass -File .\package.ps1
```

产物：

| 文件 | 说明 |
| --- | --- |
| `target\release\singularity.exe` | 直接构建产物 |
| `release\v1.5.0\Singularity.exe` | package.ps1 复制出的便携版 |
| `release\v1.5.0\Singularity-win64.zip` | 便携发行包（内含 README + docs/code_specification.md + assets/preview.png） |

### 1.2 版本/依赖

- Rust edition 2021。
- `wgpu 0.20.1`、`egui 0.28.1`、`egui-wgpu 0.28.1`。
- `windows 0.58`（Win32 API）。
- `serde` / `serde_json`：配置持久化。
- `png`：快照输出。
- `pollster`：同步等待 wgpu 初始化。
- `bytemuck`：uniform 结构体 zerocopy。
- `winres`（build-dependency）：exe 图标/版本资源。
- **没有** LLM、HTTP、DPAPI、文件 Agent、命令执行相关依赖。

`Cargo.toml` 中的 release profile：

```toml
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```

### 1.3 运行数据目录

`log::data_dir()` 优先使用 `%APPDATA%\Singularity`；如果无法创建，则回退到 exe 同目录的 `data`。

| 文件 | 路径 |
| --- | --- |
| 设置 | `%APPDATA%\Singularity\settings.json` |
| 日志 | `%APPDATA%\Singularity\singularity.log` |
| 设置解析失败备份 | `settings.json.bad` |
| 设置临时文件 | `settings.json.tmp` |

调试/测试时可通过设置 `APPDATA` 到临时目录，避免覆盖真实用户配置；项目内的测试实例都采用这种方式。

---

## 2. 项目结构

```text
Singularity/
├─ src/
│  ├─ main.rs             程序入口，模块声明，--snapshot / --icons
│  ├─ app.rs              窗口生命周期、消息循环、帧调度、托盘、状态机集成
│  ├─ panel.rs            360×500 egui 设置面板、预渲染、动画、CPU 透镜
│  ├─ stats.rs            304×364 悬停系统状态 HUD、待办、媒体按钮、输入路由
│  ├─ ui.rs               共享 egui 画布、主题、颜色、控件与动画辅助
│  ├─ sysmon.rs           CPU/GPU/内存/电池 1s 采样线程
│  ├─ fun.rs              趣味互动状态机、补丁窗口、图标精灵、涟漪/白洞
│  ├─ desktop_icons.rs    桌面 ListView 枚举/可见性/移动/inpaint/精灵裁切
│  ├─ capture.rs          GDI BitBlt 桌面捕获
│  ├─ region.rs           全屏框选漫游区域
│  ├─ win.rs              分层窗口、点击穿透、WDA、DPI、全屏检测、单实例、图标
│  ├─ render.rs           wgpu 设备/管线/HDR/合成/桌面纹理/回读
│  ├─ shader.wgsl         黑洞场景、吸积盘、光子环、相机、自发光
│  ├─ composite.wgsl      桌面透镜、趣味吸入/涟漪/白洞、ACES、透明边缘
│  ├─ texture.rs          程序化吸积盘纹理
│  ├─ settings.rs         Settings/Reminder/Todo/Note、serde、原子持久化
│  ├─ log.rs              本地日志
│  └─ snapshot.rs         离屏渲染与 PNG 输出
├─ assets/
│  ├─ app.ico             应用/托盘图标（build.rs 使用）
│  └─ preview.png         README 预览图
├─ docs/
│  └─ code_specification.md  本文件
├─ release/                  预编译产物（git 忽略，由 package.ps1 生成）
│  └─ v1.5.0/
│     ├─ Singularity.exe
│     └─ Singularity-win64.zip
├─ build.rs               生成 exe 图标、版本、产品名
├─ package.ps1            构建 + 生成 release\v<version> + zip
├─ Cargo.toml
├─ Cargo.lock
├─ .cargo/config.toml     crates 镜像配置（rsproxy.cn）
├─ .gitignore             target/release/out/日志/备份文件
└─ README.md              用户向说明（1.5.0）
```

### 2.1 各文件职责与关键入口

| 文件 | 关键类型/函数 | 说明 |
| --- | --- | --- |
| `main.rs` | `main()` | 日志/panic hook/DPI awareness；分派 `--snapshot`、`--icons`、GUI |
| `app.rs` | `App`, `run()`, `App::tick()`, `App::render()` | 主状态机与调度中枢 |
| `panel.rs` | `SettingsPanel`, `PanelActions`, `build_ui()`, `prewarm()` | 设置面板 |
| `stats.rs` | `StatsOverlay`, `build_ui()`, `prewarm()` | 悬停 HUD |
| `ui.rs` | `EguiCanvas`, `install_fonts/style()`, 主题常量, 控件 | 面板/HUD 共用 UI 层 |
| `sysmon.rs` | `SystemMonitor`, `StatsSnapshot`, `GpuCounter` | 采样线程 |
| `fun.rs` | `FunController`, `PatchWindow`, `SpriteWindow` | 趣味状态机与两个辅助分层窗口 |
| `desktop_icons.rs` | `DesktopIcon`, `PatchPixels`, `SpritePixels`, `list_icons()`, `move_icon()` | 桌面图标跨进程操作 |
| `render.rs` | `Renderer`, `FrameParams`, `Uniforms` | wgpu 渲染核心 |
| `shader.wgsl` | `vs_main` / `fs_main` | 黑洞场景 |
| `composite.wgsl` | `vs_main` / `fs_main` | 透镜/合成 |
| `settings.rs` | `Settings`, `Reminder`, `Todo`, `Note` | 配置模型 |
| `win.rs` | `LayeredSurface`, `set_click_through()`, `exclude_from_capture()` | Win32 封装 |
| `capture.rs` | `DesktopCapture` | 屏幕捕获 |
| `region.rs` | `RegionSelector` | 框选区域 |
| `snapshot.rs` | `run(args)` | 离屏快照 |
| `log.rs` | `init()`, `info/warn/error` | 日志 |

---

## 3. 总体架构

### 3.1 进程/线程模型

```text
┌──────────────────────────────────────────────────────────┐
│ UI 主线程（单线程消息循环）                               │
│  - App::tick()：状态机、窗口位置、捕获、渲染、输入        │
│  - PeekMessage/DispatchMessage + MsgWaitForMultipleObjects│
│  - 设置面板/HUD 的 egui 渲染也在主线程                    │
└──────────────┬───────────────────────────────────────────┘
               │ 共享 Arc<Mutex<StatsSnapshot>>
┌──────────────▼───────────────────────────────────────────┐
│ SystemMonitor 采样线程                                     │
│  - 1s 周期：CPU/GPU/内存/电池                             │
│  - PDH GPU 计数器（可选）                                 │
└──────────────────────────────────────────────────────────┘
```

没有网络线程、Agent 线程或子进程。所有 UI、渲染、桌面图标操作都在主线程执行；只有系统监控在一个后台线程中。

### 3.2 分层窗口

应用由多个独立的分层窗口组成：

| 窗口类 | 创建者 | 作用 | 截屏排除 |
| --- | --- | --- | --- |
| `SingularityPetWindowV1` | `App::create()` | 黑洞本体、透镜、拖拽、点击 | 默认 WDA；调试可关闭 |
| `SingularitySettingsPanelV1` | `App::ensure_panel()` | egui 设置面板 | 默认 WDA |
| `SingularityStatsWindowV1` | `App::ensure_stats()` | 悬停系统 HUD + 媒体按钮 | 默认 WDA |
| `SingularityRegionSelectorV1` | `region.rs` / `App` | 全屏框选 | 默认 WDA |
| `SingularityIconPatchV1` (patch) | `fun.rs::PatchWindow` | 图标原位置背景补丁 | **不设 WDA**（故意进入透镜捕获） |
| `SingularityIconPatchV1` (sprite) | `fun.rs::SpriteWindow` | 图标精灵飞入/飞出 | **不设 WDA**（故意进入透镜捕获） |

所有窗口使用 `WS_EX_LAYERED` + `WS_POPUP`；宠物/面板/HUD 使用 `WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`。像素通过 `UpdateLayeredWindow` 写入预乘 alpha BGRA。

### 3.3 数据流

```text
Desktop (BitBlt, excluding WDA windows)
        │
        ▼
capture.rs (BGRA) ──► render.rs upload texture
        │
        ▼
shader.wgsl scene (HDR Rgba16Float)
        │
        ▼
composite.wgsl (lens warp + backdrop + fun effects + ACES)
        │
        ▼
Bgra8Unorm target ──copy_texture_to_buffer──► pixels
        │
        ▼
win::LayeredSurface.update ──UpdateLayeredWindow──► desktop
```

设置面板/HUD 共用 `ui::EguiCanvas` 的独立 egui 渲染链：

```text
egui Context.run(raw input)
  └─ tessellate
     └─ egui_wgpu::Renderer → Rgba8Unorm target
        └─ copy_texture_to_buffer → readback
           └─ CPU 颜色通道转换 RGBA→BGRA
              └─ （可选）缩放/淡入/透镜弯折
                 └─ LayeredSurface.update
```

---

## 4. 运行时生命周期与帧调度

### 4.1 入口

`main()`：

1. `log::init()` 初始化日志（`%APPDATA%\Singularity\singularity.log`）。
2. `std::panic::set_hook` 将 panic 写入日志。
3. `win::set_process_dpi_aware()`（Per-Monitor V2）。
4. 参数分派：
   - `--snapshot` → `snapshot::run(&args)`，不创建桌面窗口。
   - `--icons` → `desktop_icons::debug_dump()`，打印桌面 ListView item。
   - 否则 `app::run()`。

### 4.2 `app::run()` 主循环

1. 单实例互斥：`Global\SingularityBlackHolePet`；已有实例则直接退出（返回 Ok）。
2. `App::create()` 创建窗口、wgpu 渲染器、加载设置等。
3. 将 `App*` 写入宠物窗口的 `GWLP_USERDATA`，供 wndproc 取回。
4. `after_created()`：注册全局热键、托盘、点击穿透、预创建面板/HUD、`prewarm()`。
5. 循环：
   - `PeekMessageW` + `TranslateMessage` + `DispatchMessageW` 处理所有 Win32 消息。
   - 到时间则 `App::tick()`，按 `frame_interval()` 安排下一帧。
   - `MsgWaitForMultipleObjectsEx` 等待输入或下一帧（最多 200ms）。
6. `shutdown()`：保存设置、销毁面板/HUD/托盘/区域选择、释放资源。

### 4.3 帧间隔

`App::frame_interval()`：

- 用户隐藏或全屏隐藏：200ms。
- 以下任一活动时 16ms（约 60fps）：
  - 拖拽、点击脉冲、悬停权重、淡入淡出、退出；
  - 趣味状态机活动或涟漪强度 >0.01；
  - 设置面板/HUD 处于活动状态。
- 否则 33ms（约 30fps）。

### 4.4 `App::tick()` 顺序（关键）

`app.rs:504` 起：

1. `dt = clamp(now-last_frame, 0, 0.10)`，`clock += dt`。
2. `check_reminders()`：到期提醒/闹钟、托盘气泡、提示音、脉冲。
3. 专注结束检测：`focus_until_unix <= now` → 清理 `focus_until_unix`/`focus_reminder_id`。
4. 退出淡出 / 初次淡入。
5. 每 900ms 检查前台是否全屏；全屏隐藏时销毁框选、关闭面板、取消趣味、隐藏 HUD。
6. 不可见时清理拖拽并 `fun.abort()` 后返回。
7. `update_dpi_if_needed()`。
8. 计算工作区、鼠标位置、半径、命中半径、点击穿透状态。
9. HUD 悬停判定（2.4R 或鼠标在当前 HUD 上）；设置 1.3s linger；`ensure_stats()`/`set_layout()`/`set_open()`。
10. 构造 `FunContext`，调用 `fun.tick()`；把 `absorb/ripple_age/ripple_strength/white` 写入 App；处理托盘 notice；应用趣味移动输出。
11. 位置更新：
    - 拖拽：跟随鼠标 + 偏移并 clamp；
    - 趣味移动：覆盖位置，冻结 wander；
    - 正常漫游：鼠标靠近/面板/HUD 打开时冻结，否则 `update_wander()`。
12. 更新 `visual_time`、点击脉冲、悬停视差权重。
13. `clamp_position()`。
14. `render(now)`：宠物 wgpu 渲染 + 写分层窗口。
15. `render_panel()`：面板 egui 渲染/动画/应用 `PanelActions`。
16. `render_stats()`：HUD 渲染/动画/媒体 action。
17. 如果有面板/趣味/HUD 活动：`win::bring_to_top(pet)`，保证宠物在辅助窗口之上。
18. `poll_panel_close()` / `poll_region_select()`。
19. 每 20s 左右保存一次漫游后的位置。

### 4.5 宠物位置/漫游

- `center_x/center_y` 是屏幕像素坐标，窗口尺寸 `width = 6R`（`WINDOW_SCALE=6`）。
- `radius = width/6`；命中半径 `1.30R`。
- 默认起始位置：工作区 65% 宽、56% 高（未保存位置时）。
- 漫游：
  - `update_wander()` 在 `wander_area()` 内随机选目标；
  - 速度 `wander_speed`（0–120）；
  - 鼠标靠近（<2.4R）、设置面板打开、HUD 打开时冻结；
  - 趣味移动时完全覆盖漫游。
- `clamp_position()`：保证窗口完整落在工作区。
- 位置保存：`settings.x/y`，松开拖拽/退出/定时保存。
- 双击核心：切换 `wander`。
- 拖拽：`WM_LBUTTONDOWN` 命中 + `WM_MOUSEMOVE`；移动超过阈值后不触发单击；`WM_LBUTTONUP` 结束。

---

## 5. Win32 窗口与消息路由

### 5.1 窗口类注册

`App::create()` 依次注册：

- `SingularityPetWindowV1` → `wndproc`
- `SingularitySettingsPanelV1` → `panel_wndproc`
- `SingularityRegionSelectorV1` → `region_wndproc`
- `SingularityStatsWindowV1` → `stats_wndproc`
- 趣味 patch/sprite 类由 `fun.rs::register_patch_class` 用 `Once` 注册，`DefWindowProcW`。

每个 wndproc 都从 `GWLP_USERDATA` 取回 `App*`；面板/区域/统计窗口在创建后由 App 写入该指针。`WM_NCDESTROY` 时清空。

### 5.2 宠物窗口消息（`App::handle_message`，`app.rs:971`）

| 消息 | 行为 |
| --- | --- |
| `WM_NCHITTEST` | 核心/拖拽状态返回 `HTCLIENT`，否则 `HTTRANSPARENT` |
| `WM_MOUSEACTIVATE` | `MA_NOACTIVATE` |
| `WM_ERASEBKGND` | 返回 1，交给分层窗口 |
| `WM_SETCURSOR` | 根据命中区域设置手型/箭头 |
| `WM_LBUTTONDOWN` | 命中核心则开始拖拽；取消趣味/HUD；`SetCapture` |
| `WM_MOUSEMOVE` | 更新鼠标；拖拽中移动窗口 |
| `WM_LBUTTONUP` | 结束拖拽；单击/双击判定；`handle_click_release` |
| `WM_RBUTTONUP` / `WM_CONTEXTMENU` | 打开/关闭面板 |
| `WM_HOTKEY` | `Ctrl+Alt+B` 显示/隐藏 |
| `WM_TRAY` | 托盘回调，见 `handle_tray()` |
| `WM_DPICHANGED` | 销毁面板/HUD、按新 DPI 调整窗口、重新定位 |
| `WM_DISPLAYCHANGE` | 取消趣味、clamp 位置 |
| `WM_QUERYENDSESSION` / `WM_ENDSESSION` | 保存设置、允许关闭 |
| `WM_CLOSE` | `begin_exit()` |
| `WM_DESTROY` | `PostQuitMessage` |

### 5.3 面板消息（`App::handle_panel_message`，`app.rs:2035`）

- `WM_CHAR` / `WM_IME_CHAR` → `panel.text_input(char)`。
- `WM_KEYDOWN` / `WM_KEYUP` → `panel.key_input(vk, pressed)`（映射 Backspace/Tab/Enter/Escape/Home/End/方向键/Delete）。
- `WM_MOUSEMOVE` → client 坐标 / DPI → `panel.pointer_moved`。
- `WM_LBUTTONDOWN` / `WM_LBUTTONUP` → `panel.pointer_button`。
- `WM_MOUSEWHEEL` → `panel.mouse_wheel(delta)`。
- `WM_ERASEBKGND` → 1；`WM_NCHITTEST` → `HTCLIENT`。
- 面板窗口是**可激活**的（不使用 `WS_EX_NOACTIVATE`），以便文本输入。

### 5.4 HUD 消息（`App::handle_stats_message`，`app.rs:2002`）

- HUD 窗口使用 `SingularityStatsWindowV1` + `stats_wndproc`，不设置 `WS_EX_TRANSPARENT`，因此可以接收鼠标点击。
- `WM_MOUSEACTIVATE` → `MA_NOACTIVATE`（点击不抢焦点）。
- `WM_MOUSEMOVE` → `StatsOverlay::pointer_moved`。
- `WM_LBUTTONDOWN/UP` → `StatsOverlay::pointer_button`。
- 点击媒体按钮后，`StatsOverlay::render` 返回 `Option<String>` action；`App::render_stats()` 调用 `send_media_key()`。
- 鼠标离开后，App 通过 1.3 秒 linger 保持 HUD；`hide_stats()` 调用 `pointer_gone`。

### 5.5 趣味辅助窗口

- `PatchWindow`：显示 inpainted 背景补丁，覆盖原图标区域，拦截点击（无 `WS_EX_TRANSPARENT`）。
- `SpriteWindow`：显示图标精灵，使用 `WS_EX_TRANSPARENT` 点击穿透。
- 两者都是 `WS_EX_LAYERED | TOOLWINDOW | TOPMOST | NOACTIVATE`，创建后用 `SetWindowPos(hwnd, pet_hwnd, ...)` 放到宠物窗口下方；sprite 在 patch 之后创建，因此位于 patch 之上、宠物之下。
- **故意不调用 `win::exclude_from_capture`**，以便黑洞的桌面捕获包含补丁/精灵，让 composite 透镜能看到它们。这会使趣味动画出现在截图/录屏中，是设计取舍。

---

## 6. 配置数据模型与持久化

### 6.1 `Settings`（`settings.rs:36`）

| 字段 | 类型 | 默认 | 作用 |
| --- | --- | --- | --- |
| `radius_dip` | f32 | 52.0 | 黑洞半径 DIP；clamp 28–92 |
| `wander` | bool | true | 自由漫游 |
| `wander_speed` | f32 | 22.0 | 漫游速度；clamp 0–120 |
| `lens_strength` | f32 | 0.60 | 桌面透镜强度；clamp 0–2 |
| `accent` | String | `#FFE8D2` | 吸积光色；非法回退 |
| `custom_area` | Option<[i32;4]> | None | 框选漫游区域（left, top, right, bottom） |
| `fullscreen_hide` | bool | true | 全屏自动隐藏 |
| `paused` | bool | false | 暂停动画/漫游/趣味 |
| `x`, `y` | Option<i32> | None | 宠物中心屏幕坐标 |
| `reminders` | Vec<Reminder> | [] | 提醒/闹钟/专注截止 |
| `reminder_sound` | bool | true | 提醒提示音 |
| `todos` | Vec<Todo> | [] | 待办 |
| `notes` | Vec<Note> | [] | 便签 |
| `focus_until_unix` | Option<u64> | None | 专注截止 Unix 秒 |
| `focus_minutes` | u32 | 25 | 上次专注分钟数 |
| `focus_reminder_id` | Option<u32> | None | 专注关联提醒 id |
| `intro_seen` | bool | false | 首次提示是否已看 |
| `fun_enabled` | bool | false | 自动吸收桌面图标总开关 |
| `fun_interval_seconds` | u32 | 90 | 自动吸收基础间隔（30–300 秒，实际 ±18% 抖动） |

`Note { id, text, created_unix }`；`Reminder { id, due_unix, label, repeat_daily, enabled }`；`Todo { id, title, done }`。

### 6.2 加载/保存

- `settings::load()`：
  - 读取 `settings.json`；
  - serde 反序列化 → `clamp()`；
  - 解析失败：复制为 `settings.json.bad`，回退默认；
  - 文件不存在：默认。
- `settings::save()`：
  - 复制并 `clamp()`；
  - 写 `settings.json.tmp`；
  - `rename` 到 `settings.json`（原子替换）；
  - 失败只写日志。
- 保存时机：面板动作 `commit`、关闭面板、托盘/设置退出、提醒到期、专注开始/停止、位置定时、拖拽结束、关机消息等。

### 6.3 当前 JSON 示例

```json
{
  "radius_dip": 52.0,
  "wander": true,
  "wander_speed": 22.0,
  "lens_strength": 2.0,
  "accent": "#FFE8D2",
  "custom_area": null,
  "fullscreen_hide": true,
  "paused": false,
  "x": 826,
  "y": 582,
  "reminders": [],
  "reminder_sound": true,
  "todos": [],
  "notes": [],
  "focus_until_unix": null,
  "focus_minutes": 25,
  "focus_reminder_id": null,
  "intro_seen": true,
  "fun_enabled": false,
  "fun_interval_seconds": 90
}
```

> 旧的 `agent_*`、`fun_ripples` 字段会被 serde 忽略；1.3/1.4 已移除 LLM 与这些配置。`fun_interval_seconds` 在 1.5.0 重新引入为可控的自动吸收间隔。

---

## 7. 渲染子系统

### 7.1 渲染器初始化

`Renderer::new(width, height)`（`render.rs`）：

- `wgpu::Instance` → 选择 HighPerformance adapter → device/queue。
- 场景目标：`Rgba16Float`（HDR）。
- 最终目标：`Bgra8Unorm`，带 `RENDER_ATTACHMENT | COPY_SRC`，用于读回。
- 吸积盘纹理：`texture.rs::build_disk_texture()`。
- 两条管线：
  - `scene_pipeline`：读 uniform + 盘纹理，输出 HDR 场景；
  - `composite_pipeline`：读场景 HDR + 桌面背景纹理，输出最终 BGRA。
- 全屏三角形：`pass.draw(0..3, 0..1)`，顶点 shader 生成覆盖屏幕的三角形。
- 读回：`copy_texture_to_buffer` → `map_async` → `device.poll(Maintain::Wait)`，按 padded bytes per row 复制到 `Renderer::pixels`。
- `Renderer::resize` 在半径/DPI 变化时重建纹理和 bind group。

### 7.2 `FrameParams` 与 `Uniforms`

`FrameParams`（`render.rs:26`）由 `App::render()` 填充：

| 字段 | 含义 |
| --- | --- |
| `time` | 视觉时间（受暂停影响） |
| `hole_radius` | 物理像素半径 |
| `yaw/pitch/roll/distance` | 相机 |
| `disk_inner/outer/thickness/brightness` | 吸积盘 |
| `exposure/glow` | 曝光/泛光 |
| `pulse/pulse_age` | 点击脉冲 |
| `fade` | 显示/退出淡入淡出 |
| `hover_x/hover_y/motion_amount` | 悬停视差 |
| `lens_strength` | 桌面透镜强度 |
| `accent` | 线性空间吸积光色 |
| `fun` | `[absorb, ripple_age, ripple_strength, white]` |
| `focus` | 专注呼吸强度 |
| `debug` | UI/调试层选择 |

`Uniforms`（`render.rs:14`，`#[repr(C)]` + `Pod`）必须与 `shader.wgsl`/`composite.wgsl` 的 struct 完全一致：

```rust
resolution: [f32;2], time: f32, hole_radius: f32,
camera: [f32;4], disk: [f32;4], look: [f32;4],
motion: [f32;4], flags: [f32;4], capture: [f32;4],
accent: [f32;4], fun: [f32;4], extra: [f32;4]
```

关键语义：

- `flags = [debug, lens_strength, 1.0, 0.0]`；`flags.y` 是透镜强度。
- `capture = [offset_x, offset_y, width, height]`；`capture.z/w` 为 0 表示关闭桌面透镜（调试/无捕获）。
- `fun.x` = 吸收强度；`fun.y` = 涟漪年龄（-1 表示关闭）；`fun.z` = 涟漪强度；`fun.w` = 白洞闪光。
- `extra.x` = 专注呼吸强度（0–0.15 左右）。

### 7.3 场景 shader（`shader.wgsl`）

- 全屏三角形 + 光线/吸积盘近似。
- 输出 HDR 光强、颜色、alpha；包含：
  - 事件视界（黑色不透明）；
  - 光子环/亮环；
  - 吸积盘近侧/远侧、多普勒/透镜抬升；
  - 外侧柔光；
  - 点击脉冲（`look.z/w`）；
  - 专注呼吸：`raw_light *= 1.0 + pulse*0.55 + focus*0.65`。
- 不直接采样桌面；桌面透镜在 composite pass。

### 7.4 合成 shader（`composite.wgsl`）

核心步骤：

1. 计算屏幕中心距离 `d = length(pixel - center)/hole_radius`。
2. 如果 capture 有效：
   - 采样桌面背景纹理；
   - 径向位移 `bend = lens_strength * R / max(d,0.16) * (0.30+0.70*falloff²) * outer_soft`；
   - 趣味吸收：`absorb * R * 0.62 / max(d,0.28) * (1-smoothstep(0.72,2.35,d))`；
   - 吸收旋涡：切向偏移；
   - 引力波：`ripple_age >= 0` 时，半径 `1.02 + age*2.35` 的扩散波，正弦位移 + 环光；
   - 桌面 UV = `(pixel + dir*bend + tangent*swirl + offset)/capture_size`；越界渐隐，避免窗口边缘硬切。
3. 场景与背景按预乘 alpha 合成。
4. 趣味暖光/涟漪环光叠加。
5. 白洞闪光：核心变白 + 光环，alpha 同步。
6. 色调映射（ACES）、伽马、dither、透明边缘淡出。

**注意**：补丁/精灵窗口不设 WDA，所以会进入 capture，被上述透镜弯折；宠物/面板/HUD 设 WDA，不会进入自身捕获。

### 7.5 桌面捕获

`capture.rs::DesktopCapture`：

- 使用 GDI `BitBlt` 从屏幕 DC 复制 BGRA。
- `Renderer` 维护一个与窗口同尺寸的 backdrop 纹理；每帧（或按需）捕获窗口所在区域并上传。
- `win::exclude_from_capture(hwnd)` 设置 `WDA_EXCLUDEFROMCAPTURE`；如果环境变量 `SINGULARITY_ALLOW_CAPTURE=1`，跳过 WDA 并关闭桌面透镜（`renderer.disable_backdrop()` / capture meta 0），避免递归捕获。
- 捕获失败会记录 warning，并保持 capture meta 关闭。

### 7.6 输出与帧转储

- 最终 pixels 为 BGRA 预乘 alpha；`LayeredSurface::update()` 调用 `UpdateLayeredWindow`。
- `SINGULARITY_DUMP_FRAME=<path>`：在第 90 帧左右把最终 layered pixels 写成 PNG（`App` 中实现）。
- `--snapshot`：离屏渲染，不捕获桌面；可配合 `--lens-test` 使用合成网格背景验证透镜。

---

## 8. 设置面板（`panel.rs` + `ui.rs`）

### 8.1 尺寸与视觉

- `PANEL_WIDTH_DIP = 360`
- `PANEL_HEIGHT_DIP = 500`
- 透明外边距 `CARD_MARGIN = 12`，Frame 内边距 `CARD_INNER = 16`，圆角 20。
- 内容宽度 `CONTENT_WIDTH = 304`，滚动区固定高度 `SCROLL_HEIGHT = 300`，页脚固定贴底。
- 背景玻璃：`BG = rgba(14,18,27,246)`，白色 5.5% 描边；顶部仅保留一条细强调线，无右上角光晕。
- 强调色由 `settings.accent` 决定；每帧更新 `egui` selection/描边/光标颜色。
- 字体：优先 `C:\Windows\Fonts\msyh.ttc`，否则 `simhei.ttf`。
- 视觉组件（`ui.rs`）：分段页签、拨动开关、细滑杆、迷你勾选框、胶囊按钮、文字链接、细滚动条、标题光条、header 黑洞图标。
- 页签与页脚使用 `ui.rs` 的共享绘制函数，不再直接依赖 egui 默认 Button/Checkbox 外观。

### 8.2 页签与页面

`SettingsPage`：

1. **外观**
   - 半径 28–92 DIP（滑块，右侧数值）。
   - 透镜 0–2。
   - 8 个吸积光色预设 + `#RRGGBB` 输入 + 应用。
   - “恢复默认”在页脚。
2. **行为**
   - 自由漫游、速度 0–120。
   - 范围：整个工作区 / 框选区域。
   - 暂停动画、全屏自动隐藏。
   - **桌面互动**：`自动吸收桌面图标` 拨动开关 + `吸收间隔` 30–300 秒滑块 + `演示吸收` 按钮。
   - 说明文字：30 秒高频、300 秒低频、实际 ±18% 抖动；开启总开关后才自动吸收，演示按钮始终可用。
   - 文案说明引力波为默认效果。
3. **提醒**
   - 专注计时：25/45/60 分钟快捷按钮；进行中显示剩余分钟 + “结束专注”。
   - 快速提醒：+5/15/30/60 分、自定义 1–180 分、添加。
   - 时钟闹钟：HH:mm + 每天重复 + 添加。
   - 提醒列表：启用/关闭、删除、剩余时间、每日标记、提示音开关。
4. **待办**
   - 便签区：多行输入、“记下”、逐条删除；最多 100 条。
   - 待办：单行输入、添加、勾选、删除。
   - 页脚：重置位置、隐藏、退出、恢复默认（二次点击确认）。

### 8.3 `PanelActions` 合同

`panel.rs` 中的 `PanelActions`：

| 字段 | 类型 | 触发 |
| --- | --- | --- |
| `close` | bool | × |
| `radius_dip/lens_strength/accent` | Option | 外观 |
| `wander/wander_speed/paused/fullscreen_hide` | Option | 行为 |
| `set_work_area/select_region` | bool | 行为 |
| `fun_enabled` | Option<bool> | 自动吸收总开关 |
| `fun_interval_seconds` | Option<u32> | 自动吸收间隔 30–300 秒 |
| `fun_demo` | bool | “演示吸收” |
| `add_reminder/add_alarm` | Option | 提醒页 |
| `set_reminder_enabled/set_reminder_sound/remove_reminder` | Option | 提醒列表 |
| `start_focus/stop_focus` | Option/bool | 专注 |
| `add_todo/toggle_todo/remove_todo` | Option | 待办 |
| `add_note/remove_note` | Option | 便签 |
| `reset_position/hide/exit/reset` | bool | 页脚 |
| `commit` | bool | 滑块/开关保存标记 |

`App::apply_panel_actions()` 负责实际修改 Settings、调用 `fun.abort()/request_demo()`、启动/停止专注、更新 HUD 媒体等。

### 8.4 打开/关闭动画

- `set_open(true, now)`：`open_amount=0`，`last_now=now`，`page=Appearance`。
- `render()`：
  - `dt = clamp(now-last_now, 0, 0.25)`。
  - 打开速度 `1/0.52`，关闭速度 `1/0.26`。
  - `scale = 0.12 + 0.88*ease_out(open_amount)`（0.8 之后直接 1.0）。
  - `alpha = ease_out(open_amount)`（0.8 之后直接 1.0）。
  - 窗口位置：从 `spit_x/y - size/2`（黑洞中心）插值到 `final_x/y`。
  - 完全关闭：隐藏窗口、`cache_valid=false`。
- **动画缓存**：动画期间只第一帧渲染 egui + readback；之后复用 `raw_pixels`，只做区域缩放/淡入 + CPU 透镜弯折；`cache_valid=true`。
- **CPU 透镜弯折**：`compose_present()` 对每个目标像素计算相对黑洞中心的 `d`；`d<3.05` 时施加径向 bend + 切向 swirl；`warp_amount = 1-smoothstep(0.20,0.82,open_amount)`，完全展开后消失。
- **预渲染**：`prewarm()` 在启动时以隐藏状态渲染完整面板（`prewarming=true` 抑制 ShowWindow），预热 egui 字体/纹理/GPU 管线。
- **面板位置**：`App::panel_position()` 优先右侧/左侧，gap 14 DIP；垂直居中并 clamp 到工作区；面板不遮挡核心。

### 8.5 面板渲染/输入细节

- 创建：`App::ensure_panel()` 用 `PANEL_CLASS` 创建隐藏窗口，设置 `GWLP_USERDATA=App*`；`SettingsPanel::new(hwnd, dpi, device, queue)`。
- 共享画布 `ui::EguiCanvas` 管理 `egui::Context`、`egui_wgpu::Renderer`、`Rgba8Unorm` 目标、256 字节对齐 readback 与 RGBA→BGRA 转换。
- `raw.events` 来自 App 转发的 Win32 输入；`raw.focused=true`；`pixels_per_point=dpi`。
- 输入映射：转发 `WM_CHAR/WM_IME_CHAR`、常用虚拟键、鼠标/滚轮；修饰键在 `key_input` 时通过 `GetKeyState` 读取（支持 Ctrl+Enter、Shift+方向键、Ctrl+Backspace 等）。
- 面板是**可激活**窗口（区别于 pet/HUD），因此可以接受键盘输入。
- 滚动区使用细实心滚动条（3 DIP 轨道、6 DIP 内边距），仅在内容溢出时出现；页面固定按钮宽度按 295 DIP 可用宽度设计。

---

## 9. 悬停系统状态 HUD（`stats.rs` + `sysmon.rs`）

### 9.1 尺寸与位置

- `WIDTH_DIP = 304`，`HEIGHT_DIP = 364`。
- 出现 0.95s，收起 0.42s；scale 0.28→1、alpha 0→1；窗口从黑洞中心移动到最终位置。
- 最终位置 `App::stats_position()`：
  - gap = `3.3R + 24*dpi`；
  - 优先右侧、其次左侧、其次下方、再上方；
  - 最终 clamp 到工作区（margin 12*dpi）。
- 悬停判定：`cursor_dist <= 2.4R` 或鼠标在已打开 HUD 矩形内；每次命中刷新 `stats_linger_until = clock + 1.3`；离开后 1.3s 再 `hide_stats()`。HUD 右上角 × 可立即关闭。`hide_stats` 会立即关闭并清除 linger。
- 面板打开、全屏、拖拽、趣味互动、隐藏时不显示；`hide_stats` 会立即关闭并清除 linger。
- HUD 是独立窗口，使用 `STATS_CLASS` + `App::handle_stats_message` 路由输入；不设置 `WS_EX_TRANSPARENT`。

### 9.2 UI 内容

- 顶部：脉冲状态点 + “系统状态”；右侧“实时”绿点 + “1s”采样提示 + 关闭 ×。
- 四行：CPU / GPU / 内存 / 电池；每行含彩色圆点、标签、细进度条、右侧百分比（行高 22）。
- 电池：低电量红色，充电中显示“正在充电”；无电池显示“电源/—”。
- 待办：标题 + 未完成数量；最多显示 3 条未完成项，行内圆圈点击即标记完成并立即保存；无待办显示“暂无待办”，全部完成显示“全部完成”。
- 媒体控制：无文字标签，底部四个图标按钮：上一首 / 播放 / 下一首 / 静音；悬停显示 tooltip，点击返回 `StatsAction::Media`。
- 进度条：`progress = clamp(open_amount*1.35-0.35, 0,1)`，随窗口出现从 0 填充到实际值；条形带顶部高光/端点光晕。
- 字号：标题 13、标签 10.5、数值 10.8、提示 8.8。

### 9.3 采样（`sysmon.rs`）

`SystemMonitor::new()` 启动 1s 采样线程，`Arc<Mutex<StatsSnapshot>>` 共享给 App：

- CPU：`GetSystemTimes` 的 idle/kernel/user 差分；公式 `(1 - idle_delta/total_delta)*100`，首次样本 0。
- 内存：`GlobalMemoryStatusEx` 的 `dwMemoryLoad`。
- 电池：`GetSystemPowerStatus`；`BatteryLifePercent==255` 表示未知；`ACLineStatus==1` 表示充电/交流。
- GPU：PDH 英文计数器 `\GPU Engine(*)\Utilization Percentage`：
  - `PdhOpenQueryW` + `PdhAddEnglishCounterW`；
  - 每秒 `PdhCollectQueryData` + `PdhGetFormattedCounterArrayW`；
  - 取所有实例的最大值并 clamp 0–100；
  - 计数器不可用时 `gpu=None`，HUD 显示 `—`；
  - `Drop` 关闭 PDH query。
- 采样线程 `Drop` 通过 `AtomicBool` + join 退出（最多约 100ms）。

### 9.4 HUD 渲染/预渲染

- 使用与面板相同的 `ui::EguiCanvas`：egui→wgpu→readback→`LayeredSurface`；目标 `Rgba8Unorm`。
- `StatsOverlay::render()` 返回 `Option<StatsAction>`（`Media(key)` / `Close` / `ToggleTodo { id, done }`）；`App::render_stats()` 分发到 `send_media_key()`、`hide_stats()` 或更新 `Settings.todos` 并保存。
- `prewarm()` 在启动时隐藏渲染一次，预热字体/管线。
- `compose_present()` 做缩放、淡入和 CPU 径向/旋涡透镜（`d<3.05`）；最终位置在黑洞外侧，通常不会弯折。
- `StatsOverlay::set_open`/`is_active`/`contains_screen`/`set_layout` 由 App 驱动。

---

## 10. 趣味互动（`fun.rs` + `desktop_icons.rs`）

### 10.1 配置与触发

- 配置：`Settings::fun_enabled`（行为 → 桌面互动 → 自动吸收桌面图标，默认 `false`）与 `Settings::fun_interval_seconds`（30–300 秒，默认 90）。
- `fun_enabled` 只控制**自动图标吸收**；引力波圆环是默认效果，不受此开关影响。
- 自动吸收：`idle_seconds` 初始化/重排为 `fun_interval_seconds * (0.82..1.18)`；开启开关或修改间隔会立即重排。
- 打开设置面板时 `open_panel()` 会调用 `fun.abort()`；`abort()` 仅在真正有互动/demo 待处理时重排计时器，空闲时保留倒计时，避免“打开面板就把自然吸收推迟”的隐形锁。
- “演示吸收”按钮：`PanelActions.fun_demo=true` → App 关闭面板并 `fun.request_demo()`；演示不要求打开总开关。
- 自动/演示触发还要求 `can_start`：
  - 面板未打开、HUD 未打开、未拖拽、未框选、未暂停。
- 引力波：
  - `ripples_allowed = visible && !paused`；
  - 计时器初始 5s，之后 `10..24s`；
  - 涟漪在 `FunState::Idle` 时发射，强度 0.65；
  - HUD/面板不阻止涟漪，只要宠物可见且未暂停。

### 10.2 状态机

`FunState`：

```text
Idle
  │ request_demo / auto interval / can_start
  ▼
Hunting (430 px/s，朝目标图标移动，带轻微侧摆)
  │ 到达吸收距离
  ▼
Absorbing (1.05 s)
  │ t>0.06: 显示 patch + sprite；sprite 0.55s 缩小飞向黑洞
  │ t>=1.0: 选择旅行目标
  ▼
Travel (0.92 s，二次贝塞尔曲线)
  │ 到达
  ▼
Ejecting (1.15 s)
  │ 0..0.72s: 白洞闪光 + sprite 从黑洞中心放大飞向窗口外目标位置
  │ 0.73s: 移动真实图标；销毁 patch/sprite
  ▼
Idle
```

关键常量（`fun.rs`）：

| 常量 | 值 |
| --- | --- |
| `HUNT_SPEED` | 430 px/s |
| `ABSORB_SECONDS` | 1.05 s |
| `TRAVEL_SECONDS` | 0.92 s |
| `EJECT_SECONDS` | 1.15 s |
| `EJECT_FLIGHT_SECONDS` | 0.72 s |
| 目标距黑洞最小距离 | 2.6R（选择时排除太近的图标） |
| 吸收触发距离 | `reach + 1.55R`，上限 2.4R |
| 吐出位置 | 距黑洞 `3.05R + 0.55×图标边长 + 10px` 起，随机外扩 0..0.9R；保证整个图标落在宠物窗口外 |
| 引力波年龄上限 | 2.6s |

### 10.3 目标选择（`start_hunt`）

1. `desktop_icons::list_icons(work)` 枚举可见图标。
2. 排除距宠物中心 <2.6R 的图标。
3. `desktop_icons::is_desktop_point(center)` 再次确认不是被其他窗口遮挡。
4. 按距离排序，取最近 3 个中随机一个（`rand01()*3`）。
5. `capture_patch(icon, work, 20)`：
   - 抓取图标周围 20px halo 的屏幕 patch；
   - 从原图裁出图标矩形精灵（`crop_sprite`，5px 边缘羽化）；
   - `inpaint` 生成无图标背景；
   - 失败 → `notice`、4s 提示，不启动。
6. 记录 `target/patch_pixels`，状态转 Hunting。
7. 失败（无可见图标/全被遮挡/捕获失败）→ `notice` 通过托盘提示，`demo_pending` 清除。

### 10.4 补丁窗口

`PatchWindow::new(pixels, pet)`：

- 使用 `PATCH_CLASS`（`DefWindowProcW`，`Once` 注册）。
- `WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE`，**不设** `WS_EX_TRANSPARENT`（补丁会拦截点击，避免点到不可见图标）。
- `LayeredSurface::new`，`update` 显示 inpainted 背景 patch。
- `SetWindowPos(patch, pet, ...)` 放到宠物下方。
- **不设 WDA**（见 5.5）。
- `abort/complete` 时销毁；原图标自然恢复。

### 10.5 图标精灵窗口

`SpriteWindow`：

- 输入 `SpritePixels`（图标矩形 BGRA + 羽化 alpha）。
- 使用同一个 `PATCH_CLASS`，`WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE|TRANSPARENT`（点击穿透）。
- 固定窗口尺寸 = 精灵源尺寸；`update(center_x, center_y, scale, alpha)`：
  - 按 scale 将源图缩放/居中到窗口；
  - 位置 = center - size/2；
  - `LayeredSurface::update`；
  - 首次显示后 `SetWindowPos(sprite, pet, ...)`，位于宠物下方、补丁之上。
- 吸收：从图标中心缩放到 0.06、alpha 1→0，0.55s。
- 吐出：从 `travel_target` 缩放到 1、alpha 0→1，0.72s，目标为图标 target 中心；目标位置在宠物窗口外，飞行动画全程可见。
- 0.73s 时真实 `LVM_SETITEMPOSITION` 与精灵位置对齐；白洞闪光掩盖交换；随后销毁精灵/补丁。

### 10.6 引力波

- `emit_ripple(strength)` 设置 `ripple_age=0`、`ripple_strength`。
- `copy_fx` 把 `absorb/ripple_age/ripple_strength/white` 填入 `FunOutput`。
- App 写入 `fun` uniform；composite 绘制向外扩散的弯曲圆环和折射。
- 关闭 `fun_enabled` 后涟漪仍默认存在；暂停/隐藏时不发射。

### 10.7 安全与取消

- `fun.abort()`：
  - 销毁 patch/sprite；
  - 清空目标/补丁数据；
  - 状态回 Idle；
  - 清空吸收/白洞/涟漪；
  - 仅当有进行中的互动或待处理 demo 时重排下次间隔；空闲时保留倒计时；
  - 清除 demo_pending。
- 面板打开、拖拽开始、全屏、隐藏、助手/HUD 打开（HUD 会先关闭）都会取消趣味；面板打开不再重置空闲计时器。
- 真实图标移动只在 Ejecting 0.73s 时执行一次；失败只记录 warning，移除补丁，原图标保留。
- 崩溃/强杀：patch/sprite 随进程销毁；图标未移动时保持原位置；已移动时保持新位置，不会丢失。

### 10.8 桌面 ListView 操作（`desktop_icons.rs`）

**查找 ListView**：

1. `FindWindowW("Progman")` → `FindWindowExW(Progman, "SHELLDLL_DefView")` → `FindWindowExW(DefView, "SysListView32")`。
2. 失败时 `EnumWindows` 找任意 `SHELLDLL_DefView` → 找 `SysListView32`（WorkerW 场景）。

**跨进程访问**：

- `GetWindowThreadProcessId` 取 explorer PID。
- `OpenProcess(PROCESS_VM_OPERATION|VM_READ|VM_WRITE|QUERY_INFORMATION)`。
- `VirtualAllocEx(4096)` 分配远程缓冲区。
- `SendMessageTimeoutW`（900ms，`SMTO_ABORTIFHUNG`）发送 LVM 消息。
- `WriteProcessMemory`/`ReadProcessMemory` 读写 rect/LVITEMW。
- 清理 `VirtualFreeEx`/`CloseHandle`。

**消息**：

| 消息 | 用途 |
| --- | --- |
| `LVM_GETITEMCOUNT` | item 数 |
| `LVM_GETITEMRECT` (LVIR_BOUNDS) | item 屏幕 rect（需加 ListView 窗口 rect 原点） |
| `LVM_GETITEMTEXTW` | 读 item 名称（LVITEMW.pszText 指向远程文本缓冲） |
| `LVM_SETITEMPOSITION` | 移动 item（16 位打包坐标） |
| `LVM_SETITEMPOSITION32` | 16 位失败时的 32 位回退（远程 POINT） |

**可见性**：

- `WindowFromPoint(icon center)` 必须是 ListView 或 `SysListView32/WorkerW/Progman/SHELLDLL_DefView`；
- 否则视为被其他窗口遮挡，不作为目标。

**移动**：

- 屏幕坐标 → ListView client 坐标：`screen - list_rect.left/top`。
- `LVM_SETITEMPOSITION` 后 `InvalidateRect`。
- 验证：重新读取 item rect，距目标 64px 内视为成功。
- 失败 → `LVM_SETITEMPOSITION32` → 再验证；仍失败返回 false（调用方只记录 warning，原图标保持）。
- 不修改文件内容，只改快捷方式在桌面上的布局位置。

**补丁/精灵**：

- `capture_patch(icon, work, halo=20)`：
  - patch rect = icon rect 外扩 20，clamp 到工作区；
  - `DesktopCapture` 抓取屏幕；
  - `crop_sprite` 从原图裁出图标矩形，按到边缘的距离（5px）做 alpha 羽化并预乘；
  - `inpaint`：
    - 对 icon 内部区域用左右/上下边界插值 + 0.5 混合；
    - 加轻微径向明暗和哈希噪声（使用 `wrapping_mul/wrapping_add`，修复 debug 整数溢出）；
    - 24 轮邻域模糊，alpha 固定 255；
  - 返回 `PatchPixels { rect,width,height,pixels,sprite }`。
- `debug_dump()`（`--icons`）打印工作区、ListView、item 数、可见 item。

---

## 11. 提醒、专注、待办、便签、媒体

### 11.1 提醒/闹钟

- 数据结构 `Reminder { id, due_unix, label, repeat_daily, enabled }`。
- `App::check_reminders()`（`app.rs:1639`）每帧执行：
  - `due_unix <= now` 且 `enabled`：加入 `due` 列表；
  - `repeat_daily`：循环加 86400 直到未来；一次性：移除；
  - 保存、`notify_tray("奇点提醒", ...)`、`MessageBeep`（若 reminder_sound）、`pulse_click()`。
- 添加提醒：
  - `PanelActions.add_reminder`（分钟）→ `due=now+minutes*60`；
  - `add_alarm`（HH:mm + daily）→ 按 `local_seconds_of_day()` 计算下一次触发；
  - 最大 id + 1。
- 启用/禁用/删除/提示音均为设置字段更新 + 保存。
- 专注计时本质上是一个一次性提醒 + `focus_*` 字段，见下节。

### 11.2 专注计时

- `start_focus(minutes)`：
  - clamp 1–180；
  - 若已有专注则先 `stop_focus()`（移除旧提醒）；
  - 添加 `label="专注结束 · 休息一下"` 的一次性提醒；
  - `focus_until_unix = due`、`focus_minutes=minutes`、`focus_reminder_id=Some(id)`；
  - 保存 + `pulse_click()`。
- `stop_focus()`：移除关联提醒、清空字段、保存。
- tick 中检测 `focus_until_unix <= now`：清空字段（到期提醒可能已由 check_reminders 触发）。
- 呼吸：`App::render()` 计算 `focus_breath = 0.10 + 0.05*((sin(visual_time*1.4)*0.5)+0.5)`（范围 0.10–0.15），写入 `FrameParams.focus` → `Uniforms.extra.x` → scene 光强 × `1+focus*0.65`。
- UI：提醒页 25/45/60 快捷按钮；进行中显示剩余分钟 + 结束专注。

### 11.3 待办

- `Todo { id, title, done }`；添加/勾选/删除；保存。
- 设置面板显示全部待办；悬停 HUD 只显示最多 3 条未完成项与未完成总数，点击圆圈即完成并保存。
- 无提醒时间（技术债）。

### 11.4 便签

- `Note { id, text, created_unix }`；待办页便签区多行输入 + “记下” + 删除。
- 最多 100 条（添加时移除最旧）。
- 本地持久化，不联网。

### 11.5 媒体

- HUD 媒体按钮（上一首/播放/下一首/静音）返回 `StatsAction::Media(String)`。
- `App::send_media_key()`（`app.rs:1737`）：
  - `previous → VK_MEDIA_PREV_TRACK`
  - `play_pause → VK_MEDIA_PLAY_PAUSE`
  - `next → VK_MEDIA_NEXT_TRACK`
  - `mute → VK_VOLUME_MUTE`
  - `keybd_event` down/up，日志 `media key: <action>`。
- 只发送系统媒体键，不读取曲目元数据；不同播放器兼容性不同。

---

## 12. Win32 工具层（`win.rs`）

| 函数/类型 | 说明 |
| --- | --- |
| `wide(&str) -> Vec<u16>` | UTF-16 null-terminated |
| `set_process_dpi_aware()` | Per-Monitor V2 DPI |
| `fatal_message()` | 启动失败 MessageBox |
| `cursor_pos()` | `GetCursorPos` |
| `monitor_work_area(hwnd)` | 当前显示器工作区（排除任务栏） |
| `is_fullscreen_foreground(own)` | 前台窗口是否全屏且不是自身 |
| `bring_to_top(hwnd)` | `SetWindowPos(HWND_TOPMOST,...)` |
| `set_click_through(hwnd, enabled)` | `WS_EX_TRANSPARENT` 开关 |
| `exclude_from_capture(hwnd)` | `WDA_EXCLUDEFROMCAPTURE`；`SINGULARITY_ALLOW_CAPTURE=1` 时跳过 |
| `load_app_icon()` | 从 `assets/app.ico` 解 PNG/ICO 为 HICON |
| `destroy_icon()` | 销毁 HICON |
| `single_instance(name)` | 全局命名 Mutex |
| `LayeredSurface` | 创建 DIB + `UpdateLayeredWindow`；`new(hwnd,w,h)` / `update(pixels,left,top)` |

`LayeredSurface` 使用 32bpp premultiplied BGRA DIB；`update` 设置 `ULW_ALPHA`。所有分层窗口内容必须预乘 alpha。

---

## 13. 调试、测试与可观测性

### 13.1 单元测试（`cargo test` 8 项）

| 测试 | 位置 | 验证 |
| --- | --- | --- |
| `inpaint_small_patch_does_not_overflow` | `desktop_icons.rs` | inpaint 不 panic；alpha 255 |
| `icon_sprite_crop_is_masked` | `desktop_icons.rs` | 精灵裁切尺寸/边缘 alpha |
| `hunting_reaches_icon_near_top_edge` | `fun.rs` | 顶部图标可到达触发吸收，不会永远追不上 |
| `completes_full_cycle_without_moving_real_icon` | `fun.rs` | Hunting→Absorbing→Travel→Ejecting→Idle 完整状态机，不碰真实图标 |
| `idle_ripple_works_when_fun_disabled` | `fun.rs` | 关闭图标吸收时默认涟漪仍工作 |
| `idle_abort_keeps_auto_timer` | `fun.rs` | 面板打开/空闲 abort 不会重置自然吸收倒计时 |
| `interval_setting_reschedules_when_idle` | `fun.rs` | 修改间隔后空闲计时器按新值重排 |
| `eject_radius_clears_pet_window` | `fun.rs` | 吐出落点保证整个图标在宠物窗口之外 |

### 13.2 快照/命令

```powershell
New-Item -ItemType Directory -Force target\snapshots | Out-Null

# 普通黑洞
cargo run --release -- --snapshot target\snapshots\check.png --size 1000 --time 2.0

# 透镜网格 + 趣味 shader
cargo run --release -- --snapshot target\snapshots\absorb.png --size 800 --lens-test --fun-absorb 1.0
cargo run --release -- --snapshot target\snapshots\ripple.png --size 800 --lens-test --ripple-age 0.7
cargo run --release -- --snapshot target\snapshots\white.png --size 800 --lens-test --white 1.0

# 列出可见桌面图标
.\target\release\singularity.exe --icons
```

快照参数：`--size`、`--time`、`--yaw`、`--pitch`、`--pulse`、`--lens`、`--accent`、`--lens-test`、`--fun-absorb`、`--ripple-age`、`--ripple-strength`、`--white`、`--debug`、`--dump-texture`。

### 13.3 运行时环境变量

| 变量 | 作用 |
| --- | --- |
| `APPDATA` | 设置/日志目录（测试时指向临时目录） |
| `SINGULARITY_ALLOW_CAPTURE=1` | 关闭 WDA 截屏排除，同时关闭桌面透镜，避免递归捕获 |
| `SINGULARITY_PANEL_PERF=1` | 面板每帧耗时日志 |
| `SINGULARITY_ICON_DEBUG=1` | `--icons` 时打印桌面 ListView item 矩形/可见性调试信息 |
| `SINGULARITY_DUMP_FRAME=<path>` | 第 90 帧左右输出最终分层像素 PNG |

### 13.4 日志

- 主线程日志写入 `%APPDATA%\Singularity\singularity.log`；debug 构建同时输出到 stderr。
- 关键日志：启动、GPU adapter、宠物窗口、托盘、面板/HUD 创建、面板打开/关闭、趣味目标/吸收/转移/完成/失败、图标移动、提醒、专注、媒体键、错误/panic。

---

## 14. 已知限制与清理记录

### 14.1 功能限制

- **真实图标移动尚未在当前测试环境验收**：桌面 ListView 有 41 个 item，但全部被宿主窗口遮挡，`--icons` 可见 0；需要在真实可见桌面验收补丁/精灵/LVM 移动/吐出。
- 趣味精灵使用“图标矩形 + 背景”的裁切，边缘羽化；在背景变化大时可能看到轻微矩形痕迹，白洞闪光会掩盖交接。
- 媒体控制只发系统媒体键，不读取曲目/进度；依赖播放器。
- GPU 使用 PDH `GPU Engine` 英文计数器；部分系统/驱动不可用，HUD 显示 `—`。
- 200% DPI、4K、低端 GPU、Explorer 重启未系统复测。
- 面板/ HUD 的渲染是 CPU readback；高分辨率/低端 GPU 下需复测。
- 数据目录不可写时回退到 exe 同目录 `data`。
- 没有安装器、代码签名、开机启动；发布为便携 exe/zip。
- 无 LICENSE 文件，只有 `Cargo.toml` 的 MIT 声明。

### 14.2 1.5.0 清理记录

已完成：

- 删除 `out/`：开发截图、shader 预览、临时 PowerShell 测试脚本全部移除。
- 删除 `src/composite_stylized.wgsl.bak`、`src/shader_stylized.wgsl.bak`。
- 删除 `packaging/README.txt`；便携包只在 zip 内含根 `README.md`、`docs/code_specification.md` 与 `assets/preview.png`，`release/` 目录只保留 exe/zip。
- 删除 `HANDOFF.md`、`REQUIREMENTS.md`；文档收敛为根 `README.md` + `docs/code_specification.md`。
- 移除确认无用的代码：面板/HUD 重复的 egui 画布与 readback 代码（抽为 `ui.rs`）、`App` 中只写不读的 `velocity_x/velocity_y`、`drag_moved`、`FunOutput::status`、`FunController::status()`、`win::window_rect()`、`desktop_icons::pixels_slice()` 及其 `std::slice` 导入。
- 清理 clippy：`map_or(false, ..)` → `is_some_and`、可合并 if、无意义 cast、`chunks_exact` → `as_chunks`。
- UI 收尾：HUD 增加右上角关闭 ×，linger 3s → 1.3s；移除卡片右上角光晕；HUD 增加最多 3 条未完成待办摘要与勾选完成；自动吸收间隔改为 30–300s 可调，空闲时打开面板不再重置计时器；吐出落点移到宠物窗口外并将飞行动画延长到 0.72s。
- 计时验证：临时 `APPDATA` 设 `fun_interval_seconds=30`，日志在 +39s 出现 `fun: no visible desktop icon available`，证明自然吸收计时器会触发；当前测试桌面图标被窗口遮挡，未完成真实吐出视觉验收。
- `cargo check --all-targets`、`cargo clippy --all-targets`、`cargo test` 均通过（8 项测试）。

后续可继续：

1. `cargo tree -e features` / `cargo machete` 检查未使用依赖（当前依赖均被使用）。
2. 在真实可见桌面上验收图标吸收/吐出，并复测 200% DPI / 4K / Explorer 重启。
3. 如发布，补充正式 MIT `LICENSE` 文本与图标授权说明。

### 14.3 关键坑

- **不要给 patch/sprite 设 WDA**：否则黑洞捕获不到补丁/精灵，透镜会看到原始图标，趣味视觉失效。
- **HUD 不能设 `WS_EX_TRANSPARENT`**：否则媒体按钮收不到点击。
- **面板必须可激活**：文本输入依赖焦点；宠物保持 noactivate。
- **WDA 调试开关会关闭透镜**：`SINGULARITY_ALLOW_CAPTURE=1` 只用于截图/录屏排查，不能当作产品环境。
- **不要用测试数据覆盖 `%APPDATA%\Singularity\settings.json`**；测试使用临时 `APPDATA`。
- **PowerShell 控制台中文可能显示乱码**，但源码/JSON 通常是正确 UTF-8；判断内容使用 read/grep，不要按控制台显示判断。
- **naga/WGSL 不接受 BOM**：写 shader/文档用无 BOM UTF-8。

---

## 15. 扩展指南

### 15.1 新增设置项

1. `settings.rs::Settings` 加字段，`#[serde(default)]` 保证旧配置兼容；在 `Default` 给初值。
2. 如需要范围限制，加入 `Settings::clamp()`。
3. 在 `panel.rs` 对应页面加入控件，定义 `PanelActions` 字段。
4. `App::apply_panel_actions()` 处理 action，更新 `self.settings`，必要时 `save_settings()`。
5. 若影响渲染/布局，同步更新 `App::render()`/`render_panel()`/`apply_visibility()`。
6. 更新 README/代码说明和测试。

### 15.2 新增设置页签

1. `SettingsPage` 增加枚举值，并更新 `index()` / `from_index()`。
2. 在 `build_ui()` 的 `ui::tab_bar(&["外观","行为","提醒","待办"], ...)` 标签数组中加入新名称。
3. `build_ui()` 的 `match *page` 增加分支。
4. 新增页面绘制函数（参考 `appearance/behavior`），复用 `ui::section_header/toggle_row/accent_slider/pill_button`。
5. 注意 360×500 面板中的 300 DIP 滚动区；固定宽度控件按 295 DIP 可用宽度设计，避免被滚动条裁切。

### 15.3 新增 HUD 状态

1. `StatsSnapshot` 增加字段（`sysmon.rs`），采样线程填充。
2. `StatsOverlay` 的 `build_ui()` 增加一行 `ui::stat_bar`；调整 `HEIGHT_DIP` 或行距，确保媒体按钮不被裁切。
3. 若新增输入，扩展 `handle_stats_message` 和 `StatsOverlay::pointer_*`。
4. 若新增媒体/动作，`StatsOverlay::render` 返回 action，`App::render_stats()` 处理。

### 15.4 新增趣味效果

1. `FunState` 增加状态/子阶段；更新 `tick()`。
2. 在 `FunOutput` 增加效果字段，App 写入 `FrameParams`/`Uniforms`。
3. 在 `composite.wgsl` 或 `shader.wgsl` 使用新 uniform；保持 Rust/WGSL struct 一致。
4. 若新增窗口，沿用 `PatchWindow`/`SpriteWindow` 的 `PATCH_CLASS` + `LayeredSurface` 模式；明确是否 WDA/点击穿透/z-order。
5. 保持 cancel/abort 安全：所有窗口必须在 `abort`/`Drop` 销毁。

### 15.5 新增快照参数

1. `snapshot.rs` `arg_value` 读取参数。
2. 写入 `FrameParams` 字段。
3. 如新增 uniform 字段，同时更新：Rust `Uniforms`、`FrameParams`、`shader.wgsl`、`composite.wgsl`。
4. 用 `--snapshot --lens-test` 验证。

### 15.6 新增托盘命令

1. `App::handle_tray()` 增加 `lparam` 分支。
2. 调 `open_panel()` / `show_reminders_page()` / `toggle_hidden()` 等。
3. 保持 `last_left_button`/`panel_guard_until` 逻辑，避免托盘点击立即被外部点击逻辑关闭。

---

## 16. 交接检查清单

接手后建议按顺序：

1. `git log -1` 确认基线；`git status` 确认工作区状态。
2. `cargo test`（8 项）、`cargo clippy --all-targets` 与 `cargo build --release`。
3. 用临时 `APPDATA` 启动，验证：
   - 单击核心 → 面板吐出/收起；
   - 悬停核心 → HUD 304×364、待办可勾选、媒体按钮可点、右上角 × 可关闭、离开 1.3 秒后收回；
   - 设置 → 行为 → 桌面互动：打开开关不演示，点“演示吸收”才演示；
   - 提醒/专注/便签/待办持久化；
   - 真实桌面图标吸收/吐出（需要可见图标）。
4. `--icons` 检查当前桌面可见图标数量；如果为 0，先确认窗口遮挡，再验收趣味。
5. 真实播放器上验证 HUD 媒体键。
6. 复测 200% DPI / 4K / 低端 GPU / Explorer 重启。
7. 变更后更新 `README.md`、本文件，并保持 `cargo test` 通过。

---

## 附录 A：当前关键常量速查

| 常量 | 值/位置 |
| --- | --- |
| `WINDOW_SCALE` | 6.0（`render.rs`） |
| 默认半径 | 52 DIP |
| 漫游速度 | 22（默认） |
| 透镜强度 | 0.60（默认），0–2 |
| 面板尺寸 | 360×500 DIP |
| 面板打开/关闭 | 0.52s / 0.26s |
| 面板 gap | 14 DIP |
| HUD 尺寸 | 304×364 DIP |
| HUD 出现/收回 | 0.95s / 0.42s |
| HUD 最终 gap | 3.3R + 24px |
| HUD 悬停半径 | 2.4R |
| HUD linger | 1.3s |
| 趣味 hunt 速度 | 430 px/s |
| 吸收/转移/吐出 | 1.05 / 0.92 / 1.15 s |
| 吸收触发 | reach + 1.55R，cap 2.4R |
| 自动吸收间隔 | 基础 30–300s（默认 90s），实际 ×(0.82–1.18) |
| 引力波间隔 | 初始 5s，之后 10–24s |
| 引力波上限 | 2.6s |
| 图标精灵交接 | 0.73s |
| 提醒检查 | 每帧 |
| 系统采样 | 1s |
| 主循环活动帧 | 16ms / 33ms / 200ms |

## 附录 B：面板/ HUD 坐标计算速记

- 宠物窗口：`width = radius * 6`，中心 `(center_x, center_y)`。
- 面板：`panel_w = 360*dpi`，`panel_h=500*dpi`；右侧 `pet_right + 14*dpi`，左侧 `pet_left - 14*dpi - panel_w`；y 居中并 clamp。
- HUD：`gap = 3.3*radius + 24*dpi`；右/左/下/上依次尝试；clamp 工作区（12*dpi margin）。
- 面板动画起点：`spit_x/y - panel_size/2`；终点 `final_x/y`；`t=ease_out(open_amount)`。
- HUD 动画起点：黑洞中心；终点 `final_x/y`；`ease_out`。
- 趣味吸收精灵起点：图标中心；终点 `hunt_origin`（黑洞停止点）；0.55s。
- 趣味吐出精灵起点：`travel_target`（黑洞旅行终点）；终点：目标图标中心；0.72s；落点在宠物窗口外。

## 附录 C：UI 中文化/编码约定

- 源码 UTF-8 无 BOM。
- UI 字体优先微软雅黑（`msyh.ttc`），fallback SimHei。
- 文档/日志/错误信息使用中文；代码标识符/日志前缀使用英文。
- PowerShell 控制台可能按 GBK 解码 UTF-8 文件，显示乱码不代表文件损坏。

---

*本说明对应当前 1.5.0。若后续修改代码，请同步更新本文件，至少在改动涉及窗口、uniform、设置字段、趣味状态机或 HUD 布局时更新相应章节。*
