# 项目文件格式设计 (.midiproj)

## 目标

1. 保存/恢复完整的编辑状态（轨道、音符、mixer、音源路径）
2. 支持 `midi_player <file>` 命令行直接打开
3. 格式：TOML，人类可读

## 数据结构

```rust
/// 每轨音源描述，记录实际文件路径。
#[derive(Serialize, Deserialize)]
pub enum SynthSource {
    SoundFont { path: String },
    ClapPlugin { path: String },
}

pub struct TrackData {
    // ... 现有字段 ...
    pub synth_source: SynthSource,  // 新增
}
```

`synth_index` 标记为 `#[serde(skip)]`，加载时根据 `synth_source` 动态解析。

## 加载流程

```
midi_player <file>
  ├── .mid      → MidiData::load() → 根据 config 填充 synth_source
  └── .midiproj → ProjectFile::load() → synth_source 已持久化
                                       → 按路径去重创建 synth 实例
                                       → 分配 synth_index
```

## 改动范围

| 文件 | 改动 |
|------|------|
| `midi.rs` | 添加 `SynthSource`，`TrackData` 加字段，`synth_index` 加 `#[serde(skip)]` |
| `main.rs` | 解析 `std::env::args`，传给 `build_ui` |
| `window.rs` | `build_ui(app, initial_file)` 启动时自动加载 |
