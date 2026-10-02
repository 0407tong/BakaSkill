//! 平台专有能力的**唯一收敛点**。
//!
//! 约定：任何依赖 Windows API 的调用都必须放在本模块内，并为非 Windows
//! 平台提供返回 `Err(AppError::Unsupported)` 的 stub。这样：
//!
//! 1. 跨平台编译不会因为缺少 `#[cfg(windows)]` 而失败；
//! 2. 后期落地其他平台时，改动范围被限制在本目录；
//! 3. 代码检索"哪里用了平台专有能力"有唯一答案。
//!
//! stub 必须是显式 `Err`，**不得使用 `unimplemented!()` / `panic!()`**，
//! 否则会变成"编译通过但一跑就崩"。

pub mod link;
pub mod process;
pub mod volume;

pub use link::LinkKind;
pub use volume::{VolumeInfo, VolumeKind};
