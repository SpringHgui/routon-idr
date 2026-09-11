//! 导出符号自检：如果开发包里的 Sdtapi.dll 可达，加载并断言所有封装接口的名字都能解析。
//!
//! 该测试**不需要连接读卡器**：只做 LoadLibrary + GetProcAddress，
//! 用来证明本封装里声明的符号名与真实 32 位 DLL 导出表一致。

use std::path::PathBuf;

fn find_dll() -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Ok(p) = std::env::var("SDTAPI_DLL") {
        cands.push(PathBuf::from(p));
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 本 crate 独立后位于开发包根目录的下一层，故 ../Sdtapi.dll 即根目录里的 DLL。
    cands.push(manifest.join("../Sdtapi.dll"));
    cands.push(manifest.join("../../Sdtapi.dll"));
    cands.push(manifest.join("Sdtapi.dll"));
    cands.into_iter().find(|p| p.exists())
}

#[test]
fn all_wrapped_symbols_resolve() {
    let dll = match find_dll() {
        Some(d) => d,
        None => {
            eprintln!("跳过：未找到 Sdtapi.dll（设置 SDTAPI_DLL 或放在开发包根目录）");
            return;
        }
    };
    let sdt = routon_idr::Sdt::load(&dll)
        .expect("加载 32 位 Sdtapi.dll 失败：确认已用 i686 目标构建本测试");
    let missing = sdt.check_symbols();
    assert!(
        missing.is_empty(),
        "以下封装接口在 DLL 中解析不到: {missing:?}"
    );
}
