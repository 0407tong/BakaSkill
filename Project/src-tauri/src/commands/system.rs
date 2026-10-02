//! 系统级命令：连通性探针、版本信息等。

use crate::error::AppResult;

/// `ping` 的返回结构。
/// 字段以 camelCase 序列化，与 `src/types/ipc.ts` 的 `PongPayload` 对应。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PongPayload {
    pub message: String,
    pub app_version: String,
    pub platform: String,
    pub timestamp_ms: u64,
}

use serde::Serialize;

/// 连通性探针：验证「前端 invoke -> Rust 命令 -> 序列化回传」整条链路可用。
///
/// 桌面端最常见的故障是 capabilities 权限漏配导致 invoke 报晦涩错误，
/// 保留一个不依赖任何业务状态的探针能显著缩短这类问题的排查时间。
#[tauri::command]
pub fn ping() -> AppResult<PongPayload> {
    // 记录握手：这条日志是「前端 -> IPC -> Rust」链路可用的直接证据，
    // 也是 capabilities 权限漏配时最快的定位手段。
    tracing::debug!("收到前端 ping，返回 pong");

    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    Ok(PongPayload {
        message: "pong".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        timestamp_ms,
    })
}
