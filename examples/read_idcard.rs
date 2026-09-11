//! 示例：读取身份证。
//!
//! 运行（需把 Sdtapi.dll 及依赖放 exe 同目录，或用参数/环境变量指定路径）：
//! ```text
//! cargo run --example read_idcard
//! cargo run --example read_idcard -- "D:\\sdk\\Sdtapi.dll"
//! SDTAPI_DLL=D:\\sdk\\Sdtapi.dll cargo run --example read_idcard
//! ```
//! 免驱 USB-HID 版会先 `Authenticate()` 触发射频再读（库已封装好时序）。

use std::path::{Path, PathBuf};

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

fn main() {
    let dll = match locate_dll(std::env::args().nth(1)) {
        Some(d) => d,
        None => {
            eprintln!("未找到 Sdtapi.dll：请把开发包里的 Sdtapi.dll 及依赖放 exe 同目录，或用参数/SDTAPI_DLL 指定。");
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
    println!("已加载: {}，封装接口 {} 个", dll.display(), routon_idr::supported_symbols().len());

    // 免驱 USB-HID 版首选时序；失败再退回常规端口。
    let session = match sdt.open_driverfree(routon_idr::port::DRIVER_FREE_HID, false) {
        Ok(s) => s,
        Err(_) => match sdt.open_any(&[routon_idr::port::USB_HID, routon_idr::port::AUTO, 1, 2]) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("打开设备失败: {e}");
                std::process::exit(6);
            }
        },
    };
    println!("设备已打开。SAM = {:?}", session.sam_id().ok());

    // 免驱版需先 Authenticate 触发扫描，再读。
    for _ in 0..5 {
        if session.authenticate().unwrap_or(false) {
            match session.read_id_card() {
                Ok(id) => {
                    println!("---- 身份证 ----");
                    println!("  姓名   : {}", id.name);
                    println!("  性别   : {}", id.gender);
                    println!("  民族   : {}", id.nation);
                    println!("  出生   : {}", id.birth);
                    println!("  号码   : {}", id.id_number);
                    println!("  住址   : {}", id.address);
                    println!("  签发机关: {}", id.agency);
                    println!("  有效期 : {} ~ {}", id.valid_from, id.valid_to);
                    return;
                }
                Err(e) => println!("  读取失败: {e}"),
            }
        } else {
            println!("  未检测到卡，请把身份证贴紧感应区…");
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    eprintln!("未能读到身份证。");
    std::process::exit(7);
}
