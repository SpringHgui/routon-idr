//! 底层：加载 Sdtapi.dll 并把每个导出函数解析为强类型 `extern "system"`（x86 上即 `__stdcall`）函数指针。
//!
//! 之所以用“动态加载 + GetProcAddress”，而不是链接 `Sdtapi.lib`：
//! 1. 运行期可指定 DLL 路径，方便部署；
//! 2. 依赖 DLL 与主 DLL 同目录时用 `LOAD_WITH_ALTERED_SEARCH_PATH` 一起解析；
//! 3. 名称/签名错误会在加载阶段以 `Error::Symbol` 暴露，而不是链接期。
//!
//! 这些签名与 `Sdtapi.h` 及 DLL 导出表中的 stdcall 修饰名
//! （如 `_ReadBaseInfos@36`、`_Routon_IC_HL_ReadCard@20`）逐一对齐。
#![allow(non_snake_case)]

use crate::error::{Error, Result};
use crate::win::{HMODULE, LPCSTR};
use core::ffi::{c_char, c_int, c_uint, c_void};
use core::mem::transmute_copy;

#[link(name = "kernel32")]
extern "system" {
    fn GetProcAddress(hModule: HMODULE, lpProcName: LPCSTR) -> *const c_void;
}

/// 把一批 `(导出名, 结构体字段, 函数指针类型)` 声明成结构体 + 解析逻辑。
macro_rules! ffi_table {
    ($( $name:literal => $field:ident : $ty:ty ),* $(,)?) => {
        /// 一次性解析出的全部导出函数指针。
        pub struct Api {
            $( pub $field: $ty, )*
        }

        /// 全部导出符号名，供自检/测试遍历。
        pub const SYMBOL_NAMES: &[&str] = &[ $( $name ),* ];

        impl Api {
            /// 从已加载的模块句柄解析所有符号；任一失败即返回 `Error::Symbol`。
            ///
            /// # Safety
            /// `handle` 必须是由 `LoadLibrary*` 得到且在本 `Api` 存活期间保持有效的模块句柄。
            pub unsafe fn from_handle(handle: HMODULE) -> Result<Api> {
                $(
                    let addr = {
                        let mut cname = String::from($name);
                        cname.push('\0');
                        GetProcAddress(handle, cname.as_ptr() as LPCSTR)
                    };
                    if addr.is_null() {
                        return Err(Error::Symbol { name: $name.to_string() });
                    }
                    let $field: $ty = transmute_copy(&addr);
                )*
                Ok(Api { $( $field, )* })
            }

            /// 仅返回“缺失的符号名”，用于自检而不构造完整 `Api`。
            pub unsafe fn missing_symbols(handle: HMODULE) -> Vec<&'static str> {
                let mut out = Vec::new();
                $(
                    {
                        let mut cname = String::from($name);
                        cname.push('\0');
                        let addr = GetProcAddress(handle, cname.as_ptr() as LPCSTR);
                        if addr.is_null() {
                            // 名字来自 'static 字面量
                            let leaked: &'static str = Box::leak($name.to_string().into_boxed_str());
                            out.push(leaked);
                        }
                    }
                )*
                out
            }
        }
    };
}

// ---- 设备 / 端口 ----
type FnOpenPort = extern "system" fn(i32) -> i32;
type FnVoidToInt = extern "system" fn() -> i32;
type FnMute = extern "system" fn(c_int) -> c_int;
type FnGetSamId = extern "system" fn(*mut c_char) -> c_int;

// ---- 身份证 ----
type FnReadBaseInfos = extern "system" fn(
    *mut c_char, *mut c_char, *mut c_char, *mut c_char, *mut c_char,
    *mut c_char, *mut c_char, *mut c_char, *mut c_char,
) -> c_int;
type FnReadBaseInfosPhoto = extern "system" fn(
    *mut c_char, *mut c_char, *mut c_char, *mut c_char, *mut c_char,
    *mut c_char, *mut c_char, *mut c_char, *mut c_char, *mut c_char,
) -> c_int;
type FnReadBaseMsg = extern "system" fn(*mut u8, *mut c_int) -> c_int;
type FnReadBaseMsgWPhoto = extern "system" fn(*mut u8, *mut c_int, *mut c_char) -> c_int;
type FnReadIinsndn = extern "system" fn(*mut c_char) -> c_int;

// ---- IC / Mifare 卡 ----
type FnIcReadSn = extern "system" fn(*mut c_char) -> c_int;
type FnIcRw = extern "system" fn(c_int, c_int, c_int, *mut u8, *mut u8) -> c_int;

// ---- USB 枚举 / 新款机型打开通道 ----
type FnFindAllUsb = extern "system" fn(*mut c_int, *mut c_int) -> c_int;
type FnIndexToInt = extern "system" fn(c_int) -> c_int;
type FnSetDual = extern "system" fn(c_int) -> c_int;
type FnReadAllType = extern "system" fn(c_int, *mut c_char, *mut c_char) -> c_int;
// 免驱 HID：显式寻卡 + 读原始报文 + 万能一次读全（5 个输出缓冲）。
type FnRoutonFind = extern "system" fn(*mut u8) -> c_int;
type FnRoutonReadMsg = extern "system" fn(*mut u8, *mut c_int) -> c_int;
type FnRoutonReadAllBase =
    extern "system" fn(*mut c_char, *mut c_char, *mut c_char, *mut c_char, *mut c_char) -> c_int;
// 带 iPortID 的部标低层时序（多通道机型需显式指定端口）。
type FnSdtFind = extern "system" fn(c_int, *mut u8, c_int) -> c_int;
type FnSdtSelect = extern "system" fn(c_int, *mut u8, c_int) -> c_int;
type FnSdtReadBaseMsg =
    extern "system" fn(c_int, *mut u8, *mut c_int, *mut u8, *mut c_int, c_int) -> c_int;
// 低层 Mifare（dc_* 约定：返回 0 表示成功）。
type FnDcInit = extern "system" fn(c_int, c_int) -> c_int;
type FnDcRequest = extern "system" fn(c_int, c_int, *mut c_uint) -> c_int;
type FnDcAnticoll = extern "system" fn(c_int, c_int, *mut c_uint) -> c_int;
type FnDcSelect = extern "system" fn(c_int, c_uint, *mut u8) -> c_int;
type FnDcAuth = extern "system" fn(c_int, c_int, c_int, *mut u8) -> c_int;
type FnDcRw = extern "system" fn(c_int, c_int, *mut u8) -> c_int;
type FnDcClose = extern "system" fn(c_int) -> c_int;

ffi_table! {
    "InitComm"                => init_comm:            FnOpenPort,
    "CloseComm"               => close_comm:           FnVoidToInt,
    "Authenticate"            => authenticate:         FnVoidToInt,
    "CardOn"                  => card_on:              FnVoidToInt,
    "IsFingerPrintDevice"     => is_fp_device:         FnVoidToInt,
    "Routon_GetIdrType"       => idr_type:             FnVoidToInt,
    "Routon_ShutDownAntenna"  => shutdown_antenna:     FnVoidToInt,
    "Routon_Mute"             => mute:                 FnMute,
    "GetSAMIDToStr"           => get_sam_id:           FnGetSamId,

    "ReadBaseInfos"           => read_base_infos:      FnReadBaseInfos,
    "ReadBaseInfosPhoto"      => read_base_infos_photo: FnReadBaseInfosPhoto,
    "ReadBaseMsg"             => read_base_msg:        FnReadBaseMsg,
    "ReadBaseMsgWPhoto"       => read_base_msg_wphoto: FnReadBaseMsgWPhoto,
    "ReadIINSNDN"             => read_iinsndn:         FnReadIinsndn,

    "Routon_IC_FindCard"      => ic_find:              FnVoidToInt,
    "Routon_IC_HL_ReadCardSN"=> ic_read_sn:           FnIcReadSn,
    "Routon_IC_HL_ReadCard"   => ic_read:              FnIcRw,
    "Routon_IC_HL_WriteCard"  => ic_write:             FnIcRw,

    // 新款 / USB-HID / 双通道机型用这套枚举 + 打开接口。
    "FindAllUSB"              => find_all_usb:         FnFindAllUsb,
    "GetHIDCount"             => get_hid_count:        FnVoidToInt,
    "SelectUSB"               => select_usb:           FnIndexToInt,
    "InitSDTandHIDComm"       => init_sdt_hid:         FnIndexToInt,
    "CloseSDTandHIDComm"      => close_sdt_hid:        FnIndexToInt,
    "HIDSelect"               => hid_select:           FnIndexToInt,
    "Routon_SetTransmissionChannel" => set_tx_channel: FnSetDual,
    "Routon_ReadAllTypeCardInfos"   => read_all_type:  FnReadAllType,

    // 免驱 HID 版显式寻卡/读卡（这两个仅以 stdcall 修饰名导出，故按修饰名解析）。
    "_Routon_StartFindIDCard@4" => routon_find:        FnRoutonFind,
    "_Routon_ReadBaseMsg@8"      => routon_read_msg:   FnRoutonReadMsg,
    "Routon_ReadAllBaseInfos"   => routon_read_all:    FnRoutonReadAllBase,

    // 带 iPortID 的部标低层时序。
    "SDT_StartFindIDCard"      => sdt_find:            FnSdtFind,
    "SDT_SelectIDCard"         => sdt_select:          FnSdtSelect,
    "SDT_ReadBaseMsg"          => sdt_read_msg:        FnSdtReadBaseMsg,

    // 免驱 HID 双通道时序所需（原厂示例：setTransWay(2)→InitComm(1001)→SetTransmissionChannel）。
    "iDR210HID_setTransWay"    => set_trans_way:       FnIndexToInt,
    "Routon_RepeatRead"        => repeat_read:         FnSetDual,

    // 低层 Mifare 接口（dc_* 约定：0 表示成功）。
    "dc_init"                  => dc_init:             FnDcInit,
    "dc_request"               => dc_request:          FnDcRequest,
    "dc_anticoll"              => dc_anticoll:         FnDcAnticoll,
    "dc_select"                => dc_select:           FnDcSelect,
    "dc_authentication_passaddr" => dc_auth:           FnDcAuth,
    "dc_read"                  => dc_read:             FnDcRw,
    "dc_write"                 => dc_write:            FnDcRw,
    "dc_halt"                  => dc_halt:             FnDcClose,
    "dc_exit"                  => dc_exit:             FnDcClose,
}
