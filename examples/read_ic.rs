//! 示例：读取 Mifare IC 卡（13.56MHz）。
//!
//! ```text
//! cargo run --example read_ic
//! cargo run --example read_ic -- "D:\\sdk\\Sdtapi.dll"
//! ```
//! 注意：`KeyType` 用 PC/SC 约定 KeyA=0x60 / KeyB=0x61（库已封装）；
//! 读块前先 `ic_request()` 只寻卡激活，别先读 SN（会 halt 卡）。

use std::path::{Path, PathBuf};
use routon_idr::KeyType;

fn locate_dll(cli: Option<String>) -> Option<PathBuf> {
    if let Some(p) = cli {
        return Some(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("SDTAPI_DLL") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let c = dir.join("Sdtapi.dll");
            if c.exists() {
                return Some(c);
            }
        }
    }
    if Path::new("Sdtapi.dll").exists() {
        return Some(PathBuf::from("Sdtapi.dll"));
    }
    None
}

fn hexdump(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let dll = match locate_dll(std::env::args().nth(1)) {
        Some(d) => d,
        None => {
            eprintln!("未找到 Sdtapi.dll：请放 exe 同目录或用参数/SDTAPI_DLL 指定。");
            std::process::exit(3);
        }
    };
    let sdt = match routon_idr::Sdt::load(&dll) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("加载失败: {e}");
            std::process::exit(4);
        }
    };
    let session = match sdt.open_driverfree(routon_idr::port::DRIVER_FREE_HID, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("打开设备失败: {e}");
            std::process::exit(6);
        }
    };

    let key = [0xFFu8; 6]; // 出厂默认密钥
    for _ in 0..8 {
        if let Some(card) = session.ic_find().unwrap_or(None) {
            println!("寻到卡: {}  卡号: {}", card.kind_label(), card.sn);
            for blk in 0..3i32 {
                let _ = session.ic_request(); // 只寻卡激活
                match session.ic_read_block(0, blk, KeyType::KeyA, &key) {
                    Ok(data) => println!("  扇区0 块{blk}: {}", hexdump(&data)),
                    Err(e) => println!("  扇区0 块{blk}: 读失败 {e}（可能密钥非默认/已加密）"),
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            return;
        }
        println!("  未寻到 IC 卡，重试…（放一张 13.56MHz M1/UltraLight 卡）");
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    eprintln!("未找到 IC 卡。");
    std::process::exit(7);
}
