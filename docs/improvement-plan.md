# 改进计划

本文记录当前代码的主要问题，以及分阶段的改法。阶段 A 只抽共享内核、拆窗口模块；**不把钢琴卷帘和鼓卷帘合成一个 View**。

## 现状

GTK4 MIDI 编辑器 / 播放器：钢琴卷帘、鼓卷帘、多轨回放（SoundFont / CLAP / SFZ）、实体 MIDI 键盘、`.mid` 与 `.midiproj`。

```
window.rs
  → RollStack（每轨一个 PianoRollWidget 或 DrumRollWidget）
  → Player
       → Sequencer + TrackSynth + AudioEngine
```

已经比较稳的部分：采样点精度的 sequencer、热替换时尽量不重触发、live MIDI 与序列器的音符所有权、`TrackId`、力度曲线与主音量无锁、关闭前冲静音。

## 问题

### 1. 卷帘代码双份复制（阶段 A）

`piano_roll/` 与 `drum_roll/` 各约 2500 行，`input.rs` 都超过 1200 行。真正不同的只有：

- 绘制（音高格 vs 鼓行、音符条 vs 鼓点）
- 左侧键盘（钢琴键 vs 鼓名）
- Y 轴（MIDI pitch vs drum map row）
- 默认 channel（0 vs 9）、新音符吸附/时值、命中宽度

数据、拖拽、框选、Put、快捷键、滚动几乎相同。`RollStack` 还为每轨各持一份 `MidiData` 再互相同步。

### 2. `window.rs` 过大（阶段 A）

`build_ui` 约 1900 行，文件对话框、轨道面板、播放定时器、MIDI 路由、插件 GUI、力度曲线全焊在一起。

### 3. 混音字段未接到音频（阶段 B）

`TrackMixerSettings` 有 `volume_db` / `pan`，项目文件也会存，但播放路径只用 mute/solo。`preview_synth` 仍每块渲染，试听已经打到对应 `TrackSynth`。

### 4. 采样率与音源生命周期（阶段 B）

`Player` 在设备协商后仍把 `sample_rate` 写成 `44100`；CLAP/SFZ 可能在真实采样率确定前激活；SFZ 丢掉 MIDI channel；CLAP bundle `Box::leak`；`sfizz-rs` 是本地 path 依赖。

### 5. 音频线程抢 `Mutex`（阶段 C）

CPAL 回调里对 sequencer / synths / live_notes 使用 `try_lock`。UI 持锁时会丢块。`Player` 里还有多处 `lock().unwrap()`。

### 6. 编辑器能力缺口（阶段 C/D）

没有 Undo/Redo。MIDI 模型几乎只有 NoteOn/Off。`set_bpm` 会整表覆盖 tempo map。拍号写死 4/4，循环永远开。

### 7. 文档与工程（阶段 D）

README 未覆盖鼓轨、SFZ、`.midiproj`、力度曲线。命令行实际只开 `.midiproj`。没有 LICENSE。配置里的 `default.sf2` 是相对路径。

## 阶段划分

| 阶段 | 做什么 |
|------|--------|
| **A** | 抽出卷帘共享内核；钢琴/鼓仍为两个 View。拆 `window.rs` |
| **B** | 混音 volume/pan、去掉死 preview synth、对齐采样率 |
| **C** | Undo、音频命令队列替换 Mutex |
| **D** | 音色/CC、循环区间、文档和依赖可移植 |

## 阶段 A 设计

```
src/roll/                 共享内核（不是 View）
  types.rs                EditMode / DragState / Theme / 吸附
  viewport.rs             时间轴 + 两种 Y 轴换算
  layout.rs               MelodicLayout / DrumLayout（Y、channel、命中）
  state.rs                数据、选择、Put、回调
  view.rs                 RollView trait（两套 widget 共用 API）
  input.rs                拖拽 / 框选 / 快捷键（经 Layout 分叉）
  renderer.rs             拍号格、播放头、选区（Y 交给 Layout）

src/piano_roll/           钢琴 View：snapshot + 钢琴绘制/键盘
src/drum_roll/            鼓 View：snapshot + 鼓绘制/侧栏
src/roll_stack.rs         仍按轨切换两个 View
src/window/
  mod.rs                  build_ui 编排
  helpers.rs              TrackUi、MIDI 口匹配、轨道列表重建
  header.rs               标题栏控件
  track_panel.rs          浮动轨道面板
  velocity_panel.rs       力度曲线面板
  overlay.rs              浮动面板拖拽
  css.rs                  样式
  tracks.rs               轨道增删改 / 乐器选择
  midi_devices.rs         实体 MIDI 输入
  files.rs                打开 / 保存 / 导出
  playback.rs             播放、BPM、预览、插件 GUI
  modes.rs                选择 / 打字键盘
```

分叉点集中在 `RollLayout`：`y_to_pitch` / `y_to_lane`、框选是否按行、垂直移动如何改 pitch、默认 channel、新音符时值、命中矩形最小宽度。
