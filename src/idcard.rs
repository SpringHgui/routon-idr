//! 身份证读取相关的高层数据结构与解析。
//!
//! 说明：`Sdtapi.dll` 的便捷接口（`ReadBaseInfos*`）在返回时会把各字段以
//! GBK（CP936）写入调用方提供的缓冲区；本模块负责安全地拷贝、按 NUL 截断、
//! 并用 `win::gbk_to_string` 解码为 Rust `String`。

use crate::error::Error;
use crate::win::gbk_to_string;

/// 一张居民身份证（含港澳台居住证走的是同一套 `ReadBase*` 解析）的结构化信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdCard {
    /// 姓名
    pub name: String,
    /// 性别（“男”/“女”）
    pub gender: String,
    /// 民族
    pub nation: String,
    /// 出生日期，格式 `CCYYMMDD`（如 `19961123`）
    pub birth: String,
    /// 公民身份号码
    pub id_number: String,
    /// 住址
    pub address: String,
    /// 签发机关
    pub agency: String,
    /// 有效期开始（`CCYYMMDD`）
    pub valid_from: String,
    /// 有效期结束（`CCYYMMDD`）
    pub valid_to: String,
    /// 若使用带照片接口，照片被写盘后的路径（`<dir>/photo.bmp`）
    pub photo_path: Option<String>,
}

impl IdCard {
    /// 由出生日期与性别拼一个便于展示的简短摘要。
    pub fn summary(&self) -> String {
        format!(
            "{}（{}·{}）  {}  有效期至 {}",
            self.name, self.gender, self.nation, self.id_number, self.valid_to
        )
    }
}

/// 从以 NUL 结尾的原始缓冲区提取子串（不做编码转换）。
pub(crate) fn c_str_bytes(buf: &[u8]) -> &[u8] {
    match buf.iter().position(|&b| b == 0) {
        Some(i) => &buf[..i],
        None => buf,
    }
}

/// 从 NUL 结尾缓冲区解码为 `String`（GBK -> UTF-8）。
pub(crate) fn c_str_gbk(buf: &[u8]) -> String {
    gbk_to_string(c_str_bytes(buf))
}

/// 把字段缓冲区集合解析成 [`IdCard`]。缓冲区顺序与 SDK 形参顺序一致。
pub(crate) fn build_id_card(fields: [&[u8]; 9], photo_path: Option<String>) -> IdCard {
    let [name, gender, nation, birth, code, address, agency, from, to] = fields;
    IdCard {
        name: c_str_gbk(name),
        gender: c_str_gbk(gender),
        nation: c_str_gbk(nation),
        birth: c_str_gbk(birth),
        id_number: c_str_gbk(code),
        address: c_str_gbk(address),
        agency: c_str_gbk(agency),
        valid_from: c_str_gbk(from),
        valid_to: c_str_gbk(to),
        photo_path,
    }
}

/// 校验身份证号长度是否像样（18 位，或老式 15 位）。仅作提示性判断。
pub fn looks_like_id_number(s: &str) -> bool {
    s.len() == 18 || s.len() == 15
}

/// 低层 `SDT_ReadBaseMsg` 的原始返回：文字报文（GB13000/UCS-2）与照片 WLT 字节。
#[derive(Debug, Clone)]
pub struct SdtRaw {
    /// 身份证文字原始报文（未解析）。
    pub text: Vec<u8>,
    /// 照片 WLT 编码原始字节（需 WltRS.dll 解码为 BMP）。
    pub photo_wlt: Vec<u8>,
}

pub(crate) fn buffer_too_small(field: &'static str) -> Error {
    Error::BufferTooSmall { field }
}
