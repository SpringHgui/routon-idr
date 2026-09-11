//! 最小化的 kernel32 绑定：动态加载 DLL、解析导出符号、GBK/UTF-8 编码转换。
//!
//! 这里刻意不依赖 `windows` / `winapi` / `libloading` 等外部 crate，
//! 只用 `extern "system"` 直接声明所需函数并链接 `kernel32`，
//! 使整个封装保持“零第三方依赖”，也便于离线构建。
#![allow(non_snake_case, dead_code)]

use core::ffi::{c_void, c_int};
use core::ptr;

pub type HANDLE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type DWORD = u32;
pub type LPCSTR = *const i8;
pub type LPCWSTR = *const u16;
pub type LPWSTR = *mut u16;
pub type BOOL = i32;

#[link(name = "kernel32")]
extern "system" {
    /// 加载 DLL。返回 HMODULE，失败返回 null（0）。
    pub fn LoadLibraryExW(lpLibFileName: LPCWSTR, hFile: HANDLE, dwFlags: DWORD) -> HMODULE;
    /// 卸载由 LoadLibrary* 加载的模块。
    pub fn FreeLibrary(hLibModule: HMODULE) -> BOOL;
    /// 按 ANSI 名称取导出函数地址，失败返回 null。
    fn GetProcAddress(hModule: HMODULE, lpProcName: LPCSTR) -> *const c_void;
    /// 最近一次错误的错误码。
    fn GetLastError() -> DWORD;

    /// 多字节 (GBK/CP936 等) -> UTF-16。
    fn MultiByteToWideChar(
        CodePage: DWORD,
        dwFlags: DWORD,
        lpMultiByteStr: LPCSTR,
        cbMultiByte: c_int,
        lpWideCharStr: LPWSTR,
        cchWideChar: c_int,
    ) -> c_int;
    /// UTF-16 -> 多字节 (UTF-8 传 CP_UTF8=65001)。
    fn WideCharToMultiByte(
        CodePage: DWORD,
        dwFlags: DWORD,
        lpWideCharStr: LPCWSTR,
        cchWideChar: c_int,
        lpMultiByteStr: *mut i8,
        cbMultiByte: c_int,
        lpDefaultChar: LPCSTR,
        lpUsedDefaultChar: *mut BOOL,
    ) -> c_int;
}

// LoadLibraryExW 标志：让被加载 DLL 使用“其自身所在目录”作为依赖搜索的起始，
// 这样与 Sdtapi.dll 同目录的 Dewlt.dll / SavePhoto.dll / WltRS.dll 才能被找到。
pub const LOAD_WITH_ALTERED_SEARCH_PATH: DWORD = 0x0000_0008;

pub const CP_UTF8: DWORD = 65001;
/// 简体中文 (GBK)。
pub const CPGBK: DWORD = 936;

/// 将 UTF-8 的 `&str` 转为以 NUL 结尾的 UTF-16 Vec，供 W 系 API 使用。
pub fn utf16z(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// 通过 kernel32 把 GBK (CP936) 字节序列解码成 Rust `String`。
/// 遇到非法字节做替换处理，绝不 panic。
pub fn gbk_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        // 1) GBK -> UTF-16，先取长度。
        let wlen = MultiByteToWideChar(
            CPGBK,
            0,
            bytes.as_ptr() as LPCSTR,
            bytes.len() as c_int,
            ptr::null_mut(),
            0,
        );
        if wlen <= 0 {
            // 解码失败则退化为有损 UTF-8 解释，至少不丢信息量。
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wbuf = vec![0u16; wlen as usize];
        let wlen2 = MultiByteToWideChar(
            CPGBK,
            0,
            bytes.as_ptr() as LPCSTR,
            bytes.len() as c_int,
            wbuf.as_mut_ptr() as LPWSTR,
            wlen,
        );
        if wlen2 <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wbuf.truncate(wlen2 as usize);
        String::from_utf16_lossy(&wbuf)
    }
}

/// 把 Rust `&str` 编码成 GBK (CP936) 字节，供 SDK 的 ANSI 形参（如照片保存目录）使用。
/// 转换失败时退回原始 UTF-8 字节，保证不 panic。
pub fn gbk_from_str(s: &str) -> Vec<u8> {
    if s.is_empty() {
        return Vec::new();
    }
    let w: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let n = WideCharToMultiByte(
            CPGBK,
            0,
            w.as_ptr() as LPCWSTR,
            w.len() as c_int,
            ptr::null_mut(),
            0,
            ptr::null(),
            ptr::null_mut(),
        );
        if n <= 0 {
            return s.as_bytes().to_vec();
        }
        let mut buf = vec![0u8; n as usize];
        let n2 = WideCharToMultiByte(
            CPGBK,
            0,
            w.as_ptr() as LPCWSTR,
            w.len() as c_int,
            buf.as_mut_ptr() as *mut i8,
            n,
            ptr::null(),
            ptr::null_mut(),
        );
        if n2 <= 0 {
            return s.as_bytes().to_vec();
        }
        buf.truncate(n2 as usize);
        buf
    }
}

/// 供 Windows 加载使用的宽字符路径转换（无 std 依赖场景备用）。
pub fn last_error() -> u32 {
    unsafe { GetLastError() }
}
