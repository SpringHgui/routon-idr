//! 纯逻辑单元测试：不依赖硬件，验证编码解码与解析辅助函数。

use routon_idr::idcard::looks_like_id_number;
use routon_idr::win::{gbk_from_str, gbk_to_string};

#[test]
fn gbk_roundtrip_chinese() {
    let s = "张三·男·汉族 住址：某某路1号";
    let bytes = gbk_from_str(s);
    let back = gbk_to_string(&bytes);
    assert_eq!(back, s, "GBK 编解码应可逆");
}

#[test]
fn gbk_decode_empty_is_empty() {
    assert_eq!(gbk_to_string(&[]), "");
}

#[test]
fn id_number_length_check() {
    assert!(looks_like_id_number("692474199611237822"));
    assert!(looks_like_id_number("123456900101001"));
    assert!(!looks_like_id_number("123"));
}

#[test]
fn gbk_decode_invalid_bytes_does_not_panic() {
    // 非法 GBK 序列也应安全返回（有损），不 panic。
    let _ = gbk_to_string(&[0xFF, 0xFE, 0x00, 0x80]);
}
