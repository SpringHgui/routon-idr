# routon-idr

精伦电子（Routon）iDR210 / iDR200 / iDR223 等 **身份证 / Mifare IC 卡读卡器** 的
`Sdtapi.dll` 的 **零第三方依赖、类型安全** Rust 封装。独立 crate，可直接作为依赖引入或发布。

## ⚠️ 位数约束（最重要）

开发包里的 `Sdtapi.dll` 及其依赖 `Dewlt.dll / SavePhoto.dll / WltRS.dll / JpgDll.dll` **全部是 32 位**。
Windows 上 64 位进程无法加载 32 位 DLL（`LoadLibrary` 报错误码 196）。因此**必须以 `i686-pc-windows-msvc` 目标构建**。
本 crate 的 `.cargo/config.toml` 已把自身 build/test/example 默认设为 i686；若误用 64 位目标，`lib.rs` 里的
`compile_error!` 会拦下提示。

```bash
rustup target add i686-pc-windows-msvc
```

## 作为依赖引入

`Cargo.toml`：

```toml
[dependencies]
routon-idr = { path = "../sdtapi-rs" }   # 发布后改用 version；代码里以 `routon_idr::` 引用
```

在你的项目 `.cargo/config.toml` 里同样固定 32 位目标：

```toml
[build]
target = "i686-pc-windows-msvc"
```

运行期把 `Sdtapi.dll` 及依赖放到可执行文件同目录（或用环境变量 `SDTAPI_DLL` 指定路径）。

## 快速上手

```rust
use routon_idr::{Sdt, port, KeyType};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let sdt = Sdt::load("Sdtapi.dll")?;                 // 或 Sdt::load_default()
// 免驱 USB-HID 版首选时序；也可用 open/open_any 走常规端口
let session = sdt.open_driverfree(port::DRIVER_FREE_HID, false)?;

// 身份证：免驱版需先 Authenticate() 触发射频，再读
if session.authenticate()? {
    let id = session.read_id_card()?;
    println!("{} {}", id.name, id.id_number);
}

// Mifare IC 卡：读块前先 ic_request() 只寻卡激活
if let Some(card) = session.ic_find()? {
    let _ = session.ic_request();
    let block = session.ic_read_block(0, 1, KeyType::KeyA, &[0xFF; 6])?;
    println!("{} -> {:?}", card.sn, block);
}
# Ok(())
# }
```

## API 一览

| 分类 | 入口 | 说明 |
| --- | --- | --- |
| 加载 | `Sdt::load(path)` / `Sdt::load_default()` | 加载 DLL + 解析全部导出符号 |
| 自检 | `Sdt::check_symbols()` | 返回解析不到的符号名（应为空） |
| 打开 | `open(port)` / `open_any(&[..])` | 传统 `InitComm`，返回 RAII `Session`（`Drop` 自动关闭） |
| 打开 | `open_driverfree(1001, dual)` | **免驱 USB-HID 版**时序：`setTransWay(2)+InitComm(1001)+双通道` |
| 打开 | `open_hid_select(i)` / `open_sdt_hid(i)` / `open_hid(i)` | 其它机型通道 |
| 枚举 | `find_all_usb()` / `hid_count()` | 返回 (SDT 数, HID 数) / HID 数量 |
| 设备 | `session.authenticate/card_on/sam_id/idr_type/is_fingerprint_device/shut_down_antenna` | 探卡 / SAM 号 / 机型 / 关天线 |
| 身份证 | `read_id_card[_with_photo]()` / `read_id_card_raw[_with_photo]()` / `read_iinsndn()` | 结构化 / 含照片 / 原始报文 / 认证密文 |
| 身份证 | `read_id_card_after_find()` / `read_card_universal(dir)` / `read_id_card_on_port(port)` | 显式寻卡 / 万能读卡 / 带 iPortID 低层时序 |
| IC 卡 | `ic_find()` / `ic_request()` / `ic_read_sn()` | 寻卡（含卡号）/ 只寻卡激活 / 读号 |
| IC 卡 | `ic_read_block[_raw]()` / `ic_write_block()` | Mifare 数据块读 / 写 |

## 真机验证过的两个关键坑

1. **免驱版身份证读取**：便捷口 `ReadBaseInfos` 冷读返回 0（未寻到卡），必须**先 `Authenticate()` 触发射频扫描再读**；
   打开也要走 `open_driverfree(1001, …)`，普通 `InitComm(20)` 对免驱版无效。
2. **Mifare 块读写**：`Routon_IC_HL_ReadCard/WriteCard` 的 `KeyType` 用 **PC/SC 约定 KeyA=0x60 / KeyB=0x61**（不是 0/1）；
   读块前用 `ic_request()` 只寻卡，别先调 `ic_read_sn()`（它读完会 halt 卡导致后续读块返回 -1）。
   低层 `dc_*` 接口对免驱 HID 无效（`dc_init` 返回 -1），那是部标/串口通道用的。

本封装只支持 **13.56MHz**（身份证 + Mifare IC）；**125kHz 的 ID/EM 卡读不了**（频段不同，SDK 也无对应接口）。

## 构建 / 测试 / 示例

```bash
cargo build                 # 默认 i686
cargo test                  # 含对真实 32 位 DLL 的符号自检（不需接读卡器）
cargo run --example read_idcard
cargo run --example read_ic
```

## 许可

MIT。见 `LICENSE-MIT`。
