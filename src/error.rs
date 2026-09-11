//! 封装层的错误类型。

use core::fmt;

/// 库本身能明确区分的几类失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 找不到 / 无法加载 Sdtapi.dll（可能路径错误或 32/64 位不匹配）。
    DllLoad { path: String, os_error: u32 },
    /// DLL 加载了，但某个导出函数解析失败（版本不符或名称错误）。
    Symbol { name: String },
    /// 设备/接口调用返回了非成功的状态码。
    Device { func: &'static str, code: i32 },
    /// 输出缓冲区不足以容纳返回数据（理论上已按文档预留，不应触发）。
    BufferTooSmall { field: &'static str },
}

/// 返回码到人类可读说明（依据二次开发接口说明 V4.2 的常见约定）。
pub fn describe(code: i32) -> &'static str {
    match code {
        1 => "成功",
        0 => "失败 / 未找到卡",
        -1 => "通信错误（无返回包）",
        -2 => "返回包校验错误",
        -3 => "接收数据长度错误",
        -4 => "读取出错",
        -5 => "写卡出错",
        -6 => "密钥/认证错误",
        -7 => "端口未打开或已被占用",
        -8 => "设备忙 / 超时",
        _ => "其它错误",
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DllLoad { path, os_error } => write!(
                f,
                "无法加载 Sdtapi.dll（{}）。Win32 错误码 {}。请确认：1) 该文件与依赖 DLL 在同目录；2) 程序以 32 位(i686)编译——64 位进程无法加载 32 位 DLL。",
                path, os_error
            ),
            Error::Symbol { name } => write!(f, "导出函数解析失败：{}（DLL 版本可能与开发包不一致）", name),
            Error::Device { func, code } => write!(
                f,
                "接口 {} 返回失败，状态码 {}（{}）",
                func,
                code,
                describe(*code)
            ),
            Error::BufferTooSmall { field } => write!(f, "字段缓冲区过小：{}", field),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;
