//! # routon-idr
//!
//! 精伦电子（Routon）iDR210 / iDR200 等身份证 / IC 卡读卡器 `Sdtapi.dll` 的
//! **零第三方依赖、类型安全** Rust 封装。
//!
//! ## 能力
//! * 动态加载 32 位 `Sdtapi.dll`（含同目录依赖 `Dewlt.dll`/`SavePhoto.dll`/`WltRS.dll`）。
//! * 打开/关闭读卡会话（RAII，自动 `CloseComm`）。
//! * 读取居民身份证信息：结构化 [`idcard::IdCard`]、含照片、以及原始报文与认证密文。
//! * 读取 Mifare IC 卡：寻卡、卡号、读/写数据块。
//! * 设备信息：SAM 模块序列号、指纹能力、机型码、静音、关天线等。
//!
//! ## 重要：位数
//! 开发包内 DLL 均为 **32 位**。宿主要以 `i686-pc-windows-msvc` 目标编译；
//! 64 位进程 `LoadLibrary` 32 位 DLL 会直接失败（错误码 196 / `%196`）。
//! 本 crate 已在 `.cargo/config.toml` 固定默认目标为 i686。
//!
//! ## 快速上手
//! ```no_run
//! use routon_idr::{Sdt, port};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let sdt = Sdt::load("Sdtapi.dll")?;      // 或 Sdt::load_default()
//! let session = sdt.open_any(&[port::USB_HID, port::AUTO, 1, 2])?;
//!
//! if session.authenticate()? {
//!     let id = session.read_id_card()?;
//!     println!("{} {}", id.name, id.id_number);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Sdt`] 与 [`Session`] 是主要入口，读操作只在会话打开后类型可用。

pub mod error;
pub mod ffi;
pub mod iccard;
pub mod idcard;
pub mod reader;
pub mod win;

#[cfg(all(windows, not(target_pointer_width = "32")))]
compile_error!(
    "Sdtapi.dll 是 32 位 DLL：请勿以 64 位目标构建 routon-idr。改用 `cargo build --target i686-pc-windows-msvc`（本 crate 的 .cargo/config.toml 已默认该目标）。"
);

pub use error::{Error, Result};
pub use iccard::{IcCard, KeyType};
pub use idcard::{IdCard, SdtRaw};
pub use reader::{port, Session, Sdt};

/// 重新导出常用的库版本信息。
pub const SDK_DOC_VERSION: &str = "二次开发接口说明 V4.2";

/// 返回底层已封装的导出符号名清单（便于自检/展示）。
pub fn supported_symbols() -> &'static [&'static str] {
    ffi::SYMBOL_NAMES
}
