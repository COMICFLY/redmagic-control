//! 纯 Rust Win32 原生 HID 枚举与 Overlapped 同步安全读写层（零 C 依赖 / 零 node-hid）：
//! - 通过 CfgMgr32 (`CM_Get_Device_Interface_ListW`) + `HidD_*` 枚举全部在线 HID 集合；
//! - 所有读写严格遵循 Overlapped + `WaitForSingleObject` + 超时 `CancelIoEx` + `GetOverlappedResult(..., TRUE)`，
//!   在函数返回前保证内核已彻底释放对缓冲区的引用，从根源杜绝悬垂指针与堆损坏 (`0xC0000374`)；
//! - 支持原生 `HidD_FlushQueue`（对齐官方 Goper.exe `0x4de812` 在 2.4G 模式下的清队列动作）。

#[derive(Debug, Clone)]
pub struct HidDeviceInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub product: String,
}

#[cfg(windows)]
mod imp {
    use super::HidDeviceInfo;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
        CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_BUFFER_SMALL, CR_SUCCESS,
    };
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HidD_FlushQueue, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid,
        HidD_GetPreparsedData, HidD_GetProductString, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS,
        HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

    fn to_wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn enumerate_hid_devices() -> Vec<HidDeviceInfo> {
        let mut out = Vec::new();
        unsafe {
            let mut hid_guid: GUID = std::mem::zeroed();
            HidD_GetHidGuid(&mut hid_guid);

            let mut buf: Vec<u16> = Vec::new();
            loop {
                let mut len: u32 = 0;
                let cr = CM_Get_Device_Interface_List_SizeW(
                    &mut len,
                    &hid_guid,
                    null(),
                    CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
                );
                if cr != CR_SUCCESS || len == 0 {
                    return out;
                }
                buf.resize(len as usize, 0);
                let cr2 = CM_Get_Device_Interface_ListW(
                    &hid_guid,
                    null(),
                    buf.as_mut_ptr(),
                    len,
                    CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
                );
                if cr2 == CR_SUCCESS {
                    break;
                }
                if cr2 != CR_BUFFER_SMALL {
                    return out;
                }
            }

            let mut idx = 0usize;
            while idx < buf.len() && buf[idx] != 0 {
                let start = idx;
                while idx < buf.len() && buf[idx] != 0 {
                    idx += 1;
                }
                let slice = &buf[start..idx];
                idx += 1; // skip NUL

                let mut wpath = Vec::with_capacity(slice.len() + 1);
                wpath.extend_from_slice(slice);
                wpath.push(0);

                // 只读查询属性句柄（desired_access = 0，无需独占即可获取 VID/PID/UsagePage）
                let h = CreateFileW(
                    wpath.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    0,
                    null_mut(),
                );
                if h == INVALID_HANDLE_VALUE || h.is_null() {
                    continue;
                }

                let mut attrib: HIDD_ATTRIBUTES = std::mem::zeroed();
                attrib.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
                if HidD_GetAttributes(h, &mut attrib) != 0 {
                    let mut usage_page = 0u16;
                    let mut usage = 0u16;
                    let mut pp: PHIDP_PREPARSED_DATA = 0;
                    if HidD_GetPreparsedData(h, &mut pp) != 0 && pp != 0 {
                        let mut caps: HIDP_CAPS = std::mem::zeroed();
                        if HidP_GetCaps(pp, &mut caps) == HIDP_STATUS_SUCCESS {
                            usage_page = caps.UsagePage;
                            usage = caps.Usage;
                        }
                        HidD_FreePreparsedData(pp);
                    }

                    let mut prod_buf = [0u16; 128];
                    let product = if HidD_GetProductString(
                        h,
                        prod_buf.as_mut_ptr() as *mut _,
                        (prod_buf.len() * 2) as u32,
                    ) != 0
                    {
                        let end = prod_buf.iter().position(|&c| c == 0).unwrap_or(prod_buf.len());
                        String::from_utf16_lossy(&prod_buf[..end]).trim().to_string()
                    } else {
                        String::new()
                    };

                    out.push(HidDeviceInfo {
                        path: String::from_utf16_lossy(slice),
                        vendor_id: attrib.VendorID,
                        product_id: attrib.ProductID,
                        usage_page,
                        usage,
                        product,
                    });
                }

                CloseHandle(h);
            }
        }
        out
    }

    pub struct WinHidDevice {
        handle: HANDLE,
        read_event: HANDLE,
        write_event: HANDLE,
        input_len: usize,
        output_len: usize,
    }

    // 句柄由外层 Mutex 独占串行访问，可安全跨线程传递
    unsafe impl Send for WinHidDevice {}

    impl WinHidDevice {
        pub fn open(path: &str) -> Result<Self, String> {
            let wpath = to_wide_null(path);
            unsafe {
                let handle = CreateFileW(
                    wpath.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    null_mut(),
                );
                if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                    let err = GetLastError();
                    return Err(format!("无法打开 HID 接口 (Win32 error={err})"));
                }

                let mut input_len = 64usize;
                let mut output_len = 64usize;
                let mut pp: PHIDP_PREPARSED_DATA = 0;
                if HidD_GetPreparsedData(handle, &mut pp) != 0 && pp != 0 {
                    let mut caps: HIDP_CAPS = std::mem::zeroed();
                    if HidP_GetCaps(pp, &mut caps) == HIDP_STATUS_SUCCESS {
                        if caps.InputReportByteLength > 0 {
                            input_len = caps.InputReportByteLength as usize;
                        }
                        if caps.OutputReportByteLength > 0 {
                            output_len = caps.OutputReportByteLength as usize;
                        }
                    }
                    HidD_FreePreparsedData(pp);
                }

                let read_event = CreateEventW(null(), 1, 0, null());
                let write_event = CreateEventW(null(), 1, 0, null());
                if read_event.is_null() || write_event.is_null() {
                    if !read_event.is_null() {
                        CloseHandle(read_event);
                    }
                    if !write_event.is_null() {
                        CloseHandle(write_event);
                    }
                    CloseHandle(handle);
                    return Err("无法创建 Win32 Overlapped 事件句柄".to_string());
                }

                Ok(Self {
                    handle,
                    read_event,
                    write_event,
                    input_len: input_len.max(64),
                    output_len: output_len.max(64),
                })
            }
        }

        pub fn flush_queue(&self) {
            unsafe {
                HidD_FlushQueue(self.handle);
            }
        }

        pub fn write_frame(&self, frame: &[u8], timeout_ms: u32) -> Result<(), String> {
            let mut buf = vec![0u8; self.output_len.max(frame.len())];
            buf[..frame.len()].copy_from_slice(frame);

            unsafe {
                ResetEvent(self.write_event);
                let mut ol: OVERLAPPED = std::mem::zeroed();
                ol.hEvent = self.write_event;
                let mut written: u32 = 0;
                let ok = WriteFile(
                    self.handle,
                    buf.as_ptr(),
                    buf.len() as u32,
                    &mut written,
                    &mut ol,
                );
                if ok != 0 {
                    return Ok(());
                }
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    return Err(format!("HID 写入失败 (Win32 error={err})"));
                }

                let w = WaitForSingleObject(self.write_event, timeout_ms);
                if w == WAIT_OBJECT_0 {
                    let mut xfer: u32 = 0;
                    if GetOverlappedResult(self.handle, &ol, &mut xfer, 1) != 0 {
                        return Ok(());
                    }
                    return Err(format!("HID 写入完成异常 (Win32 error={})", GetLastError()));
                }

                // 超时必须先 CancelIoEx 并等待内核确认撤回，确保栈上 OVERLAPPED 与 buf 绝对安全
                CancelIoEx(self.handle, &ol);
                let mut xfer: u32 = 0;
                GetOverlappedResult(self.handle, &ol, &mut xfer, 1);
                Err("HID 写入超时".to_string())
            }
        }

        /// 读取一帧输入报告：返回 `Ok(Some(bytes))` 表示读到数据，`Ok(None)` 表示在 timeout_ms 内超时无数据
        pub fn read_frame(&self, timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
            let mut buf = vec![0u8; self.input_len];
            unsafe {
                ResetEvent(self.read_event);
                let mut ol: OVERLAPPED = std::mem::zeroed();
                ol.hEvent = self.read_event;
                let mut read_bytes: u32 = 0;
                let ok = ReadFile(
                    self.handle,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut read_bytes,
                    &mut ol,
                );
                if ok != 0 {
                    buf.truncate(read_bytes as usize);
                    return Ok(Some(buf));
                }
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    return Err(format!("HID 读取失败 (Win32 error={err})"));
                }

                let w = WaitForSingleObject(self.read_event, timeout_ms);
                if w == WAIT_OBJECT_0 {
                    let mut xfer: u32 = 0;
                    if GetOverlappedResult(self.handle, &ol, &mut xfer, 1) != 0 {
                        buf.truncate(xfer as usize);
                        return Ok(Some(buf));
                    }
                    return Err(format!("HID 读取结果异常 (Win32 error={})", GetLastError()));
                }

                // 取消未完成的异步读并等待内核释放对 buf / ol 的引用
                CancelIoEx(self.handle, &ol);
                let mut xfer: u32 = 0;
                if GetOverlappedResult(self.handle, &ol, &mut xfer, 1) != 0 && xfer > 0 {
                    buf.truncate(xfer as usize);
                    return Ok(Some(buf));
                }
                Ok(None)
            }
        }
    }

    impl Drop for WinHidDevice {
        fn drop(&mut self) {
            unsafe {
                if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
                    CancelIoEx(self.handle, null());
                    CloseHandle(self.handle);
                }
                if !self.read_event.is_null() {
                    CloseHandle(self.read_event);
                }
                if !self.write_event.is_null() {
                    CloseHandle(self.write_event);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use imp::{enumerate_hid_devices, WinHidDevice};

#[cfg(not(windows))]
pub fn enumerate_hid_devices() -> Vec<HidDeviceInfo> {
    Vec::new()
}

#[cfg(not(windows))]
pub struct WinHidDevice;

#[cfg(not(windows))]
impl WinHidDevice {
    pub fn open(_path: &str) -> Result<Self, String> {
        Err("当前仅支持 Windows Win32 HID".to_string())
    }
    pub fn flush_queue(&self) {}
    pub fn write_frame(&self, _frame: &[u8], _timeout_ms: u32) -> Result<(), String> {
        Err("unsupported".to_string())
    }
    pub fn read_frame(&self, _timeout_ms: u32) -> Result<Option<Vec<u8>>, String> {
        Err("unsupported".to_string())
    }
}
