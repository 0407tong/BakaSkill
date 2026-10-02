//! 背景图命令。
//!
//! 三个动作对应设置页上的三颗按钮：选一张、恢复默认、读出当前是哪张。
//! 图会被复制进应用数据目录，因此**原图之后怎么处置都不影响背景**
//! （详见 `crate::background` 的模块说明）。

use crate::background::{self, BackgroundState};
use crate::error::AppResult;

/// 读出当前的自定义背景（`path` 为 `null` 表示正在用内置默认背景）
#[tauri::command(async)]
pub fn background_get() -> AppResult<BackgroundState> {
    background::get()
}

/// 把用户选中的图片设为背景（会复制进应用数据目录）
#[tauri::command(async)]
pub fn background_set(source: String) -> AppResult<BackgroundState> {
    background::set(&source)
}

/// 恢复内置的默认背景
#[tauri::command(async)]
pub fn background_clear() -> AppResult<BackgroundState> {
    background::clear()
}

/// 取出自定义背景的内容（data URL）。`null` 表示用内置默认背景。
#[tauri::command(async)]
pub fn background_image() -> AppResult<Option<String>> {
    background::image_data_url()
}
