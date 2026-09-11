# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 与
[语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.0] - 2026-09-11

首个版本：对精伦(Routon) `Sdtapi.dll`（iDR210 / iDR200 / iDR223 等身份证 + Mifare IC 卡读卡器）
的零第三方依赖、类型安全 Rust 封装。

### 新增

- **加载与自检**：`Sdt::load` / `Sdt::load_default` 动态加载 32 位 DLL（`LOAD_WITH_ALTERED_SEARCH_PATH`
  自动解析同目录依赖）；`Sdt::check_symbols` 校验全部导出符号可解析。
- **会话（RAII）**：`open` / `open_any` / `open_driverfree` / `open_hid_select` / `open_sdt_hid` / `open_hid`，
  `Session` 在 `Drop` 时自动关闭端口；进程级原子标志串行化单实例设备。
- **身份证读取**：结构化 `read_id_card` / `read_id_card_with_photo`（写 `photo.bmp`）、原始报文
  `read_id_card_raw[_with_photo]`、认证密文 `read_iinsndn`、显式寻卡 `read_id_card_after_find`、
  万能读卡 `read_card_universal`、带 iPortID 的低层时序 `read_id_card_on_port`。
- **Mifare IC 卡**：`ic_find` / `ic_request` / `ic_read_sn` / `ic_read_block[_raw]` / `ic_write_block`，
  `KeyType` 采用 PC/SC 约定（KeyA=0x60 / KeyB=0x61）。
- **设备信息**：`authenticate` / `card_on` / `sam_id` / `idr_type` / `is_fingerprint_device` /
  `shut_down_antenna` / `find_all_usb` / `hid_count` / `set_dual_channel` / `set_mute`。
- **编码**：GBK(CP936) 文本字段解码、照片目录入参反向编码，非法字节有损兜底、不 panic。
- **测试**：`tests/logic.rs`（编码/解析纯逻辑）、`tests/symbol_selfcheck.rs`（对真实 32 位 DLL 的符号自检）。
- **示例**：`examples/read_idcard.rs`、`examples/read_ic.rs`。

### 约束

- 仅支持 **Windows 32 位（i686-pc-windows-msvc）**；64 位进程无法加载 32 位 DLL，误用 64 位目标时
  `compile_error!` 拦截。
- 仅覆盖 **13.56MHz**（身份证 + Mifare IC）；**125kHz ID/EM 卡不支持**（硬件频段不同，SDK 无对应接口）。

[0.1.0]: #010---2026-09-11
