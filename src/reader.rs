//! 顶层封装：加载 DLL、打开/关闭读卡会话、读取身份证与 IC 卡。
//!
//! 设计要点
//! * [`Sdt`] 负责“持有已加载的 Sdtapi.dll 与解析好的函数指针”。
//! * [`Session`] 是一个 RAII 守卫，代表一次已 `InitComm` 打开的设备会话，
//!   `Drop` 时自动 `CloseComm`。所有需要设备的读操作都挂在 `Session` 上，
//!   从类型层面保证“没打开就不能读”。
//! * 该 SDK 内部是**单一全局设备**（接口不带句柄参数），因此用进程级原子标志
//!   串行化，避免同时开两个会话互相打架。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};
use crate::ffi::Api;
use crate::iccard::{IcCard, BLOCK_LEN, KEY_LEN};
use crate::idcard::{build_id_card, c_str_bytes, c_str_gbk, IdCard, SdtRaw};
use crate::win::{
    self, gbk_from_str, utf16z, HMODULE, LOAD_WITH_ALTERED_SEARCH_PATH,
};
use core::ffi::c_char;

/// 成功状态码约定：绝大多数控制/读取接口返回 `1` 表示成功。
const RET_OK: i32 = 1;

// 各输出字段缓冲区大小（均 ≥ 官方文档要求的最小字节数，留足余量）。
const BUF_NAME: usize = 128;
const BUF_GENDER: usize = 16;
const BUF_FOLK: usize = 32;
const BUF_BIRTH: usize = 16;
const BUF_CODE: usize = 64;
const BUF_ADDR: usize = 256;
const BUF_AGENCY: usize = 128;
const BUF_DATE: usize = 32;
const BUF_DIR: usize = 300;
const BUF_SAMID: usize = 64;
const BUF_ICSN: usize = 24;
const BUF_RAWMSG: usize = 1024;
const BUF_IINSNDN: usize = 256;

/// 进程级：是否已有会话占用设备。
static DEVICE_BUSY: AtomicBool = AtomicBool::new(false);

/// 常见的 `InitComm` 端口取值。具体含义随机型不同，可按顺序尝试。
pub mod port {
    /// COM1..COMn 即 1..n。
    pub const fn com(n: i32) -> i32 {
        n
    }
    /// USB-HID 接口（iDR210 常见）。
    pub const USB_HID: i32 = 20;
    /// 十六进制 0x16 的自动/特殊通道（部分型号）。
    pub const AUTO: i32 = 0x16;
    /// **免驱 USB-HID 版**的魔法端口号（原厂示例 `InitComm(1001)`）。
    pub const DRIVER_FREE_HID: i32 = 1001;
}

/// 已加载的 Sdtapi.dll 与其导出函数集合。
pub struct Sdt {
    handle: HMODULE,
    api: Api,
    path: String,
}

// 该设备为进程内单实例，且所有读操作需经 &self 串行调用底层 stdcall 函数。
// 明确不声明 Send/Sync：句柄是裸指针，跨线程共享不安全。

impl Sdt {
    /// 从指定路径加载 Sdtapi.dll。会一并解析同目录下的依赖 DLL。
    pub fn load<P: AsRef<Path>>(dll_path: P) -> Result<Sdt> {
        let path_os = dll_path.as_ref();
        let display = path_os.display().to_string();
        let wide = utf16z(
            &path_os
                .to_string_lossy()
                .into_owned(),
        );
        let handle = unsafe {
            win::LoadLibraryExW(
                wide.as_ptr(),
                core::ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if handle.is_null() {
            return Err(Error::DllLoad {
                path: display,
                os_error: win::last_error(),
            });
        }
        // 解析导出符号。
        let api = unsafe { Api::from_handle(handle) }?;
        Ok(Sdt {
            handle,
            api,
            path: display,
        })
    }

    /// 便捷：优先从环境变量 `SDTAPI_DLL`，否则在当前工作目录查找 `Sdtapi.dll`。
    pub fn load_default() -> Result<Sdt> {
        if let Ok(p) = std::env::var("SDTAPI_DLL") {
            return Sdt::load(p);
        }
        Sdt::load("Sdtapi.dll")
    }

    /// 已加载 DLL 的路径。
    pub fn path(&self) -> &str {
        &self.path
    }

    /// 自检：返回缺失的导出符号列表（应为空）。不打开设备。
    pub fn check_symbols(&self) -> Vec<&'static str> {
        unsafe { Api::missing_symbols(self.handle) }
    }

    /// 打开设备会话（`InitComm`）。失败返回带状态码的 [`Error::Device`]。
    pub fn open(&self, port_id: i32) -> Result<Session<'_>> {
        if DEVICE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::Device {
                func: "InitComm",
                code: -100,
            });
        }
        let ret = (self.api.init_comm)(port_id);
        if ret != RET_OK {
            DEVICE_BUSY.store(false, Ordering::SeqCst);
            return Err(Error::Device {
                func: "InitComm",
                code: ret,
            });
        }
        Ok(Session {
            sdt: self,
            kind: OpenKind::InitComm,
        })
    }

    /// 依次尝试候选端口，第一个成功者即返回会话。
    /// 用于端口号随机型不同的场景（文档建议“不断尝试”）。
    pub fn open_any(&self, candidates: &[i32]) -> Result<Session<'_>> {
        let mut last = None;
        for &p in candidates {
            match self.open(p) {
                Ok(s) => return Ok(s),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(Error::Device {
            func: "InitComm",
            code: 0,
        }))
    }

    /// 新款/USB 机型：枚举已连接的读卡器。返回 (SDT 通道数, HID 通道数)。
    pub fn find_all_usb(&self) -> (i32, i32) {
        let mut s: core::ffi::c_int = 0;
        let mut h: core::ffi::c_int = 0;
        (self.api.find_all_usb)(&mut s, &mut h);
        (s, h)
    }

    /// 当前连接的 USB-HID 接口 iDR210 数量（部分机型需先开端口）。
    pub fn hid_count(&self) -> i32 {
        (self.api.get_hid_count)()
    }

    /// 设置传输通道：`true` 双通道（iDR223 等），`false` 单通道。
    pub fn set_dual_channel(&self, dual: bool) {
        let _ = (self.api.set_tx_channel)(dual as core::ffi::c_int);
    }

    /// 用 SDT+HID 组合通道打开第 `index` 个设备（新款机型常见的打开方式）。
    /// 内部先 `SelectUSB(index)` 再 `InitSDTandHIDComm(index)`。
    pub fn open_sdt_hid(&self, index: i32) -> Result<Session<'_>> {
        if DEVICE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::Device {
                func: "InitSDTandHIDComm",
                code: -100,
            });
        }
        let _ = (self.api.select_usb)(index);
        let ret = (self.api.init_sdt_hid)(index);
        if ret != RET_OK {
            DEVICE_BUSY.store(false, Ordering::SeqCst);
            return Err(Error::Device {
                func: "InitSDTandHIDComm",
                code: ret,
            });
        }
        Ok(Session {
            sdt: self,
            kind: OpenKind::SdtHid(index),
        })
    }

    /// 选中第 `index` 个 HID 读卡器并用 `InitComm` 打开（iDR210 USB-HID 常见流程）。
    pub fn open_hid(&self, index: i32) -> Result<Session<'_>> {
        if DEVICE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::Device {
                func: "HIDSelect",
                code: -100,
            });
        }
        let _ = (self.api.hid_select)(index);
        let ret = (self.api.init_comm)(index);
        if ret != RET_OK {
            DEVICE_BUSY.store(false, Ordering::SeqCst);
            return Err(Error::Device {
                func: "InitComm(HID)",
                code: ret,
            });
        }
        Ok(Session {
            sdt: self,
            kind: OpenKind::Hid(index),
        })
    }

    /// **免驱 USB-HID 版**的正确打开方式：只 `HIDSelect(index)`，不调 `InitComm`
    /// （`InitComm` 是部标串口通道用的）。随后即可直接 `ReadBaseInfos` 等读卡。
    pub fn open_hid_select(&self, index: i32) -> Result<Session<'_>> {
        if DEVICE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::Device {
                func: "HIDSelect",
                code: -100,
            });
        }
        let ret = (self.api.hid_select)(index);
        // HIDSelect 返回 bool：非 0 视为选中成功。
        if ret == 0 {
            DEVICE_BUSY.store(false, Ordering::SeqCst);
            return Err(Error::Device {
                func: "HIDSelect",
                code: ret,
            });
        }
        Ok(Session {
            sdt: self,
            kind: OpenKind::HidSel,
        })
    }

    /// **免驱 USB-HID 版**的完整打开时序（对齐原厂 `DllValidate` 示例）：
    /// `Routon_RepeatRead(true)` → `Routon_Mute(true)` → `iDR210HID_setTransWay(2)`
    /// → `InitComm(1001)` →（可选）`Routon_SetTransmissionChannel(true)`。
    /// 这是 iDR210/223 免驱版能读到身份证的关键时序（普通 `InitComm(20)` 对它无效）。
    ///
    /// `dual=false` 时不调用双通道切换（单台 iDR210 免驱更常见）；`dual=true` 对应 iDR223 双通道示例。
    pub fn open_driverfree(&self, port_id: i32, dual: bool) -> Result<Session<'_>> {
        if DEVICE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Error::Device {
                func: "InitComm(免驱)",
                code: -100,
            });
        }
        // 前置设置（这些返回值不影响是否继续，仅为配置）。
        let _ = (self.api.repeat_read)(1); // Routon_RepeatRead(true)
        let _ = (self.api.mute)(1); // Routon_Mute(true)
        let _ = (self.api.set_trans_way)(2); // iDR210HID_setTransWay(2)
        let ret = (self.api.init_comm)(port_id);
        if ret != RET_OK {
            DEVICE_BUSY.store(false, Ordering::SeqCst);
            return Err(Error::Device {
                func: "InitComm(免驱)",
                code: ret,
            });
        }
        if dual {
            let _ = (self.api.set_tx_channel)(1); // Routon_SetTransmissionChannel(true)
        }
        Ok(Session {
            sdt: self,
            kind: OpenKind::InitComm,
        })
    }

    /// 设置是否静音（蜂鸣）。
    pub fn set_mute(&self, mute: bool) -> Result<()> {
        let _ = (self.api.mute)(mute as core::ffi::c_int);
        Ok(())
    }
}

impl Drop for Sdt {
    fn drop(&mut self) {
        unsafe {
            win::FreeLibrary(self.handle);
        }
    }
}

/// 会话是通过哪种方式打开的，决定 `Drop` 时用哪个关闭接口。
enum OpenKind {
    /// 传统 `InitComm` / `CloseComm`。
    InitComm,
    /// USB-HID：`HIDSelect` + `InitComm`，关闭走 `CloseComm`。
    Hid(i32),
    /// 免驱 USB-HID：仅 `HIDSelect`，无需关闭端口。
    HidSel,
    /// SDT+HID 组合：`SelectUSB` + `InitSDTandHIDComm`，关闭走 `CloseSDTandHIDComm`。
    SdtHid(i32),
}

/// 一次已打开的读卡会话；`Drop` 时自动关闭端口。
pub struct Session<'a> {
    sdt: &'a Sdt,
    kind: OpenKind,
}

impl<'a> Session<'a> {
    /// 校验/探卡：机器上是否已放置可读卡。返回 `true` 表示检测到。
    pub fn authenticate(&self) -> Result<bool> {
        Ok((self.sdt.api.authenticate)() == RET_OK)
    }

    /// 快速判断“身份证是否在机器上”。注意：仅在读卡之后用于判断是否离开，
    /// 不要用于寻卡前，否则可能导致寻卡失败。
    pub fn card_on(&self) -> Result<bool> {
        Ok((self.sdt.api.card_on)() == RET_OK)
    }

    /// 读取安全模块（SAM_V）序列号字符串。
    pub fn sam_id(&self) -> Result<String> {
        let mut buf = [0u8; BUF_SAMID];
        let ret = (self.sdt.api.get_sam_id)(buf.as_mut_ptr() as *mut c_char);
        if ret != RET_OK {
            return Err(Error::Device {
                func: "GetSAMIDToStr",
                code: ret,
            });
        }
        Ok(c_str_gbk(&buf))
    }

    /// 当前设备是否支持指纹读取。
    pub fn is_fingerprint_device(&self) -> bool {
        (self.sdt.api.is_fp_device)() == RET_OK
    }

    /// 读卡器型号代码（`Routon_GetIdrType`），不同机型返回不同整数。
    pub fn idr_type(&self) -> i32 {
        (self.sdt.api.idr_type)()
    }

    /// 关闭天线（省电/停止寻卡）。下次读身份证接口会自动重新打开。
    pub fn shut_down_antenna(&self) -> Result<()> {
        let _ = (self.sdt.api.shutdown_antenna)();
        Ok(())
    }

    /// 读取身份证，返回结构化信息（不含照片）。
    pub fn read_id_card(&self) -> Result<IdCard> {
        let (mut name, mut gender, mut folk, mut birth, mut code) = (
            [0u8; BUF_NAME],
            [0u8; BUF_GENDER],
            [0u8; BUF_FOLK],
            [0u8; BUF_BIRTH],
            [0u8; BUF_CODE],
        );
        let (mut addr, mut agency, mut from, mut to) = (
            [0u8; BUF_ADDR],
            [0u8; BUF_AGENCY],
            [0u8; BUF_DATE],
            [0u8; BUF_DATE],
        );
        let ret = (self.sdt.api.read_base_infos)(
            name.as_mut_ptr() as *mut c_char,
            gender.as_mut_ptr() as *mut c_char,
            folk.as_mut_ptr() as *mut c_char,
            birth.as_mut_ptr() as *mut c_char,
            code.as_mut_ptr() as *mut c_char,
            addr.as_mut_ptr() as *mut c_char,
            agency.as_mut_ptr() as *mut c_char,
            from.as_mut_ptr() as *mut c_char,
            to.as_mut_ptr() as *mut c_char,
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "ReadBaseInfos",
                code: ret,
            });
        }
        Ok(build_id_card(
            [
                &name, &gender, &folk, &birth, &code, &addr, &agency, &from, &to,
            ],
            None,
        ))
    }

    /// 读取身份证并把照片写到 `directory`（生成 `photo.bmp`），返回带 `photo_path` 的结构化信息。
    pub fn read_id_card_with_photo(&self, directory: &str) -> Result<IdCard> {
        let (mut name, mut gender, mut folk, mut birth, mut code) = (
            [0u8; BUF_NAME],
            [0u8; BUF_GENDER],
            [0u8; BUF_FOLK],
            [0u8; BUF_BIRTH],
            [0u8; BUF_CODE],
        );
        let (mut addr, mut agency, mut from, mut to) = (
            [0u8; BUF_ADDR],
            [0u8; BUF_AGENCY],
            [0u8; BUF_DATE],
            [0u8; BUF_DATE],
        );
        // 目录字符串以 GBK 写入并以 NUL 结尾。
        let mut dir_buf = [0u8; BUF_DIR];
        let dir_bytes = gbk_from_str(directory);
        if dir_bytes.len() >= BUF_DIR {
            return Err(crate::idcard::buffer_too_small("directory"));
        }
        dir_buf[..dir_bytes.len()].copy_from_slice(&dir_bytes);

        let ret = (self.sdt.api.read_base_infos_photo)(
            name.as_mut_ptr() as *mut c_char,
            gender.as_mut_ptr() as *mut c_char,
            folk.as_mut_ptr() as *mut c_char,
            birth.as_mut_ptr() as *mut c_char,
            code.as_mut_ptr() as *mut c_char,
            addr.as_mut_ptr() as *mut c_char,
            agency.as_mut_ptr() as *mut c_char,
            from.as_mut_ptr() as *mut c_char,
            to.as_mut_ptr() as *mut c_char,
            dir_buf.as_mut_ptr() as *mut c_char,
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "ReadBaseInfosPhoto",
                code: ret,
            });
        }
        let photo = std::path::Path::new(directory).join("photo.bmp");
        let photo_str = if photo.exists() {
            Some(photo.display().to_string())
        } else {
            Some(photo.display().to_string())
        };
        Ok(build_id_card(
            [
                &name, &gender, &folk, &birth, &code, &addr, &agency, &from, &to,
            ],
            photo_str,
        ))
    }

    /// 读取身份证原始报文（已按字段分隔，GBK/GB13000 文本），返回原始字节。
    pub fn read_id_card_raw(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; BUF_RAWMSG];
        let mut len: core::ffi::c_int = 0;
        let ret = (self.sdt.api.read_base_msg)(buf.as_mut_ptr(), &mut len);
        if ret != RET_OK {
            return Err(Error::Device {
                func: "ReadBaseMsg",
                code: ret,
            });
        }
        let n = (len.max(0) as usize).min(BUF_RAWMSG);
        Ok(buf[..n].to_vec())
    }

    /// 读取身份证原始报文并保存照片到 `directory`。
    pub fn read_id_card_raw_with_photo(&self, directory: &str) -> Result<Vec<u8>> {
        let mut buf = [0u8; BUF_RAWMSG];
        let mut len: core::ffi::c_int = 0;
        let mut dir_buf = [0u8; BUF_DIR];
        let dir_bytes = gbk_from_str(directory);
        if dir_bytes.len() >= BUF_DIR {
            return Err(crate::idcard::buffer_too_small("directory"));
        }
        dir_buf[..dir_bytes.len()].copy_from_slice(&dir_bytes);
        let ret = (self.sdt.api.read_base_msg_wphoto)(
            buf.as_mut_ptr(),
            &mut len,
            dir_buf.as_mut_ptr() as *mut c_char,
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "ReadBaseMsgWPhoto",
                code: ret,
            });
        }
        let n = (len.max(0) as usize).min(BUF_RAWMSG);
        Ok(buf[..n].to_vec())
    }

    /// 读取用于后端认证的 IINSNDN 密文字符串（身份证“防伪/认证”原始数据）。
    pub fn read_iinsndn(&self) -> Result<String> {
        let mut buf = [0u8; BUF_IINSNDN];
        let ret = (self.sdt.api.read_iinsndn)(buf.as_mut_ptr() as *mut c_char);
        if ret != RET_OK {
            return Err(Error::Device {
                func: "ReadIINSNDN",
                code: ret,
            });
        }
        Ok(c_str_gbk(&buf))
    }

    /// 万能读卡：`Routon_ReadAllTypeCardInfos`，把识别到的卡片信息以文本写入 `out_msg`，
    /// 照片写到 `photo_dir`。返回原始文本字节（GBK）。适合不确定卡型的兜底探测。
    pub fn read_card_universal(&self, photo_dir: &str) -> Result<Vec<u8>> {
        let mut msg = [0u8; BUF_RAWMSG];
        let mut dir_buf = [0u8; BUF_DIR];
        let dir_bytes = gbk_from_str(photo_dir);
        if dir_bytes.len() >= BUF_DIR {
            return Err(crate::idcard::buffer_too_small("directory"));
        }
        dir_buf[..dir_bytes.len()].copy_from_slice(&dir_bytes);
        let port = match self.kind {
            OpenKind::SdtHid(i) | OpenKind::Hid(i) => i,
            OpenKind::InitComm | OpenKind::HidSel => 0,
        };
        let ret = (self.sdt.api.read_all_type)(
            port,
            msg.as_mut_ptr() as *mut c_char,
            dir_buf.as_mut_ptr() as *mut c_char,
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_ReadAllTypeCardInfos",
                code: ret,
            });
        }
        let n = c_str_bytes(&msg).len();
        Ok(msg[..n].to_vec())
    }

    // ---------------- 免驱 USB-HID 显式寻卡 / 读卡 ----------------

    /// 显式寻卡 `Routon_StartFindIDCard`。返回原始状态码（1 成功）。
    /// 免驱 HID 版在读身份证前常需先调用它。
    pub fn find_id_card(&self) -> i32 {
        let mut iin = [0u8; 4];
        (self.sdt.api.routon_find)(iin.as_mut_ptr())
    }

    /// 免驱 HID 推荐时序：先 `Routon_StartFindIDCard` 寻卡，再用便捷口解析身份证。
    pub fn read_id_card_after_find(&self) -> Result<IdCard> {
        let f = self.find_id_card();
        if f != RET_OK {
            return Err(Error::Device {
                func: "Routon_StartFindIDCard",
                code: f,
            });
        }
        self.read_id_card()
    }

    /// 免驱 HID：寻卡后用 `Routon_ReadBaseMsg` 读身份证原始报文（GB13000 文本，含照片写盘）。
    pub fn read_id_card_raw_hid(&self, photo_dir: &str) -> Result<Vec<u8>> {
        let f = self.find_id_card();
        if f != RET_OK {
            return Err(Error::Device {
                func: "Routon_StartFindIDCard",
                code: f,
            });
        }
        let mut msg = [0u8; BUF_RAWMSG];
        let mut len: core::ffi::c_int = 0;
        // Routon_ReadBaseMsg 会把照片写到当前目录 photo.bmp；这里不传目录，交由调用方设置工作目录。
        let _ = photo_dir;
        let ret = (self.sdt.api.routon_read_msg)(msg.as_mut_ptr(), &mut len);
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_ReadBaseMsg",
                code: ret,
            });
        }
        let n = (len.max(0) as usize).min(BUF_RAWMSG);
        Ok(msg[..n].to_vec())
    }

    /// 免驱 HID 万能一次读全：`Routon_ReadAllBaseInfos(Msg, HeadPhoto, FrontCopy, BackCopy, FingerPrint)`。
    /// 返回身份证原始文本报文（GBK/GB13000 字节）。
    pub fn read_all_base_infos(&self) -> Result<Vec<u8>> {
        let f = self.find_id_card();
        if f != RET_OK {
            return Err(Error::Device {
                func: "Routon_StartFindIDCard",
                code: f,
            });
        }
        let mut msg = [0u8; BUF_RAWMSG];
        let mut head = [0u8; BUF_DIR];
        let mut front = [0u8; BUF_DIR];
        let mut back = [0u8; BUF_DIR];
        let mut fp = [0u8; BUF_DIR];
        let ret = (self.sdt.api.routon_read_all)(
            msg.as_mut_ptr() as *mut c_char,
            head.as_mut_ptr() as *mut c_char,
            front.as_mut_ptr() as *mut c_char,
            back.as_mut_ptr() as *mut c_char,
            fp.as_mut_ptr() as *mut c_char,
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_ReadAllBaseInfos",
                code: ret,
            });
        }
        let n = c_str_bytes(&msg).len();
        Ok(msg[..n].to_vec())
    }

    // ---------------- 带 iPortID 的部标低层时序（多通道机型） ----------------

    /// 在指定端口 `port` 上执行 `SDT_StartFindIDCard` + `SDT_SelectIDCard`。
    /// 返回是否成功。之后可尝试便捷解析口或 `sdt_read_id_card_raw`。
    pub fn sdt_find_and_select(&self, port: i32) -> Result<()> {
        let mut iin = [0u8; 16];
        let r = (self.sdt.api.sdt_find)(port, iin.as_mut_ptr(), 1);
        if r != RET_OK {
            return Err(Error::Device {
                func: "SDT_StartFindIDCard",
                code: r,
            });
        }
        let mut sn = [0u8; 16];
        let r = (self.sdt.api.sdt_select)(port, sn.as_mut_ptr(), 1);
        if r != RET_OK {
            return Err(Error::Device {
                func: "SDT_SelectIDCard",
                code: r,
            });
        }
        Ok(())
    }

    /// 在指定端口读身份证原始报文（文字 + 照片 WLT 字节）。
    pub fn sdt_read_id_card_raw(&self, port: i32) -> Result<SdtRaw> {
        let mut ch = vec![0u8; 512];
        let mut chlen: core::ffi::c_int = 0;
        let mut ph = vec![0u8; 40000];
        let mut phlen: core::ffi::c_int = 0;
        let r = (self.sdt.api.sdt_read_msg)(
            port,
            ch.as_mut_ptr(),
            &mut chlen,
            ph.as_mut_ptr(),
            &mut phlen,
            1,
        );
        if r != RET_OK {
            return Err(Error::Device {
                func: "SDT_ReadBaseMsg",
                code: r,
            });
        }
        let cn = (chlen.max(0) as usize).min(ch.len());
        let pn = (phlen.max(0) as usize).min(ph.len());
        Ok(SdtRaw {
            text: ch[..cn].to_vec(),
            photo_wlt: ph[..pn].to_vec(),
        })
    }

    /// 完整部标低层时序：寻卡→选卡→读原始报文。
    pub fn read_id_card_on_port(&self, port: i32) -> Result<SdtRaw> {
        self.sdt_find_and_select(port)?;
        self.sdt_read_id_card_raw(port)
    }

    // ---------------- IC / Mifare ----------------

    /// 寻 IC 卡；返回 `Some(IcCard)` 表示找到（含自动读取卡号）。
    /// 注意：内部会调用 `Routon_IC_HL_ReadCardSN`，读完会把卡 halt；若要紧接着读块，
    /// 请改用 [`ic_request`]（只寻卡不读号），避免 halt 后读不到。
    pub fn ic_find(&self) -> Result<Option<IcCard>> {
        let kind = (self.sdt.api.ic_find)();
        if kind <= 0 {
            return Ok(None);
        }
        let sn = self.ic_read_sn().unwrap_or_default();
        Ok(Some(IcCard { sn, kind_code: kind }))
    }

    /// 仅寻卡（`Routon_IC_FindCard`），不读卡号、不 halt。返回卡型码（>0 表示找到）。
    /// 读块前应调用它来激活卡片。
    pub fn ic_request(&self) -> i32 {
        (self.sdt.api.ic_find)()
    }

    /// 低层 Mifare 读块：`dc_init → dc_request → dc_anticoll → dc_select → dc_authentication → dc_read`。
    /// `dc_*` 约定返回 0 为成功。`addr` 为绝对块号(0..=63)，`auth_mode` 常用 0x60=KeyA/0x61=KeyB。
    /// 返回 (日志, 可选的 16 字节块数据)。日志里含每一步的返回码，便于定位卡在哪一步。
    pub fn dc_read_block(
        &self,
        init_port: i32,
        baud: i32,
        addr: i32,
        auth_mode: i32,
        key: &[u8; KEY_LEN],
    ) -> (Vec<String>, Option<[u8; BLOCK_LEN]>) {
        let mut log = Vec::new();
        let a = &self.sdt.api;
        let icdev = (a.dc_init)(init_port, baud);
        log.push(format!("dc_init(port={init_port},baud={baud}) -> icdev={icdev}"));
        if icdev <= 0 {
            return (log, None);
        }
        let mut tagtype: core::ffi::c_uint = 0;
        let r = (a.dc_request)(icdev, 0x52, &mut tagtype); // 0x52 = 寻所有卡
        log.push(format!("dc_request -> {r} (tagtype={tagtype:#X})"));
        if r != 0 {
            (a.dc_exit)(icdev);
            return (log, None);
        }
        let mut snr: core::ffi::c_uint = 0;
        let r = (a.dc_anticoll)(icdev, 4, &mut snr);
        log.push(format!("dc_anticoll -> {r} (snr={snr:#010X})"));
        if r != 0 {
            (a.dc_exit)(icdev);
            return (log, None);
        }
        let mut size: u8 = 0;
        let r = (a.dc_select)(icdev, snr, &mut size);
        log.push(format!("dc_select -> {r} (size={size})"));
        if r != 0 {
            (a.dc_exit)(icdev);
            return (log, None);
        }
        let mut key = *key;
        let r = (a.dc_auth)(icdev, auth_mode, addr, key.as_mut_ptr());
        log.push(format!("dc_authentication(mode={auth_mode:#X},addr={addr}) -> {r}"));
        if r != 0 {
            (a.dc_exit)(icdev);
            return (log, None);
        }
        let mut data = [0u8; BLOCK_LEN];
        let r = (a.dc_read)(icdev, addr, data.as_mut_ptr());
        log.push(format!("dc_read(addr={addr}) -> {r}"));
        let _ = (a.dc_halt)(icdev);
        let _ = (a.dc_exit)(icdev);
        if r == 0 {
            (log, Some(data))
        } else {
            (log, None)
        }
    }

    /// 读取 IC 卡序列号（会自动寻卡 + 选卡）。
    pub fn ic_read_sn(&self) -> Result<String> {
        let mut buf = [0u8; BUF_ICSN];
        let ret = (self.sdt.api.ic_read_sn)(buf.as_mut_ptr() as *mut c_char);
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_IC_HL_ReadCardSN",
                code: ret,
            });
        }
        Ok(c_str_gbk(&buf))
    }

    /// 读一个数据块（16 字节）。`sector`=SID 扇区号，`block`=BID 块号。
    pub fn ic_read_block(
        &self,
        sector: i32,
        block: i32,
        key_type: crate::iccard::KeyType,
        key: &[u8; KEY_LEN],
    ) -> Result<[u8; BLOCK_LEN]> {
        self.ic_read_block_raw(sector, block, key_type.as_i32(), key)
    }

    /// 同 [`ic_read_block`]，但 `key_type` 用原始整数（便于穷举不同固件的 KeyA/KeyB 编码）。
    pub fn ic_read_block_raw(
        &self,
        sector: i32,
        block: i32,
        key_type: i32,
        key: &[u8; KEY_LEN],
    ) -> Result<[u8; BLOCK_LEN]> {
        let mut key = *key;
        let mut data = [0u8; BLOCK_LEN];
        let ret = (self.sdt.api.ic_read)(
            sector,
            block,
            key_type,
            key.as_mut_ptr(),
            data.as_mut_ptr(),
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_IC_HL_ReadCard",
                code: ret,
            });
        }
        Ok(data)
    }

    /// 写一个数据块（16 字节）。
    pub fn ic_write_block(
        &self,
        sector: i32,
        block: i32,
        key_type: crate::iccard::KeyType,
        key: &[u8; KEY_LEN],
        data: &[u8; BLOCK_LEN],
    ) -> Result<()> {
        let mut key = *key;
        let mut data = *data;
        let ret = (self.sdt.api.ic_write)(
            sector,
            block,
            key_type.as_i32(),
            key.as_mut_ptr(),
            data.as_mut_ptr(),
        );
        if ret != RET_OK {
            return Err(Error::Device {
                func: "Routon_IC_HL_WriteCard",
                code: ret,
            });
        }
        Ok(())
    }
}

impl<'a> Drop for Session<'a> {
    fn drop(&mut self) {
        match self.kind {
            OpenKind::InitComm | OpenKind::Hid(_) => {
                (self.sdt.api.close_comm)();
            }
            OpenKind::HidSel => {
                // 免驱 HID 未开端口，无需 CloseComm。
            }
            OpenKind::SdtHid(index) => {
                (self.sdt.api.close_sdt_hid)(index);
                (self.sdt.api.close_comm)();
            }
        }
        DEVICE_BUSY.store(false, Ordering::SeqCst);
    }
}
