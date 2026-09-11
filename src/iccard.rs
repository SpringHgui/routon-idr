//! Mifare IC 卡（S50 / S70 / UltraLight）读取与写入。
//!
//! 高层接口 `Routon_IC_HL_*` 会自动完成寻卡 + 选卡，省去手动的 request/anticoll/select 流程。

/// 一张 IC 卡：卡序列号 + 寻卡返回的卡型码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcCard {
    /// 十进制/十六进制字符串形式的卡号（来自 `Routon_IC_HL_ReadCardSN`）。
    pub sn: String,
    /// `Routon_IC_FindCard` 的原始返回值，用于区分卡型（S50/S70/UltraLight 等）。
    pub kind_code: i32,
}

impl IcCard {
    /// 对寻卡返回码的可读解释（不同固件版本略有差异，以原厂文档为准）。
    pub fn kind_label(&self) -> &'static str {
        // 精伦常见约定：>0 表示找到不同类型卡片；0 / 负数表示未找到或错误。
        match self.kind_code {
            c if c <= 0 => "未识别 / 未找到卡",
            1 => "Mifare M1-S50",
            2 => "Mifare M1-S70",
            3 => "Mifare UltraLight",
            _ => "已找到卡（具体型号以返回码表为准）",
        }
    }
}

/// Mifare 一个数据块：16 字节。
pub const BLOCK_LEN: usize = 16;
/// 密钥长度：6 字节。
pub const KEY_LEN: usize = 6;

/// 密钥类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    KeyA,
    KeyB,
}

impl KeyType {
    /// 传给 SDK 的整数值。`Routon_IC_HL_ReadCard/WriteCard` 用 PC/SC 约定：
    /// KeyA = 0x60，KeyB = 0x61（实测 0/1 无效，会返回 -1）。
    pub fn as_i32(self) -> i32 {
        match self {
            KeyType::KeyA => 0x60,
            KeyType::KeyB => 0x61,
        }
    }
}
