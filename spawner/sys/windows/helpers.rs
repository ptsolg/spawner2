use crate::{Error, Result};

use winapi::shared::basetsd::{DWORD_PTR, SIZE_T};
use winapi::shared::minwindef::{DWORD, FALSE, HWINSTA, WORD};
use winapi::shared::windef::HDESK;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::processthreadsapi::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
    PROC_THREAD_ATTRIBUTE_LIST,
};
use winapi::um::securitybaseapi::{ImpersonateLoggedOnUser, RevertToSelf};
use winapi::um::userenv::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use winapi::um::winbase::{
    LogonUserW, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, STARTF_USESHOWWINDOW,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::winnt::{DELETE, HANDLE, PVOID, READ_CONTROL, WCHAR, WRITE_DAC, WRITE_OWNER};
use winapi::um::winuser::{
    CloseDesktop, CloseWindowStation, CreateDesktopW, CreateWindowStationW,
    GetProcessWindowStation, GetUserObjectInformationW, SetProcessWindowStation,
    DESKTOP_CREATEMENU, DESKTOP_CREATEWINDOW, DESKTOP_ENUMERATE, DESKTOP_HOOKCONTROL,
    DESKTOP_JOURNALPLAYBACK, DESKTOP_JOURNALRECORD, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP,
    DESKTOP_WRITEOBJECTS, SW_HIDE, SW_SHOW, UOI_NAME, WINSTA_ALL_ACCESS,
};

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::u32;

pub struct Handle(pub HANDLE);

unsafe impl Send for Handle {}

pub struct RawStdio {
    pub stdin: Handle,
    pub stdout: Handle,
    pub stderr: Handle,
}

pub struct User {
    pub token: Handle,
    pub winsta: HWINSTA,
    pub desktop: HDESK,
    pub desktop_name: Vec<u16>,
}

pub struct UserContext<'a>(&'a Option<User>);

pub struct EnvBlock {
    block: *mut u16,
    len: usize,
}

pub struct StartupInfo {
    pub base: STARTUPINFOEXW,
    _att_list: AttList,
}

struct AttList {
    ptr: *mut PROC_THREAD_ATTRIBUTE_LIST,
    len: usize,
}

const DESKTOP_ALL: DWORD = DESKTOP_CREATEMENU
    | DESKTOP_CREATEWINDOW
    | DESKTOP_ENUMERATE
    | DESKTOP_HOOKCONTROL
    | DESKTOP_JOURNALPLAYBACK
    | DESKTOP_JOURNALRECORD
    | DESKTOP_READOBJECTS
    | DESKTOP_SWITCHDESKTOP
    | DESKTOP_WRITEOBJECTS
    | DELETE
    | READ_CONTROL
    | WRITE_DAC
    | WRITE_OWNER;

pub trait IsZero {
    fn is_zero(&self) -> bool;
}

macro_rules! impl_is_zero {
    ($($type:ident)*) => ($(
        impl IsZero for $type {
            fn is_zero(&self) -> bool {
                *self == 0
            }
        }
    )*)
}

impl_is_zero!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

impl<T> IsZero for *const T {
    fn is_zero(&self) -> bool {
        self.is_null()
    }
}

impl<T> IsZero for *mut T {
    fn is_zero(&self) -> bool {
        self.is_null()
    }
}

/// Returns last os error if the value is zero.
pub fn cvt<T: IsZero>(v: T) -> Result<T> {
    if v.is_zero() {
        Err(Error::last_os_error())
    } else {
        Ok(v)
    }
}

pub fn to_utf16<S: AsRef<OsStr>>(s: S) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

impl User {
    pub fn create<T, U>(user: T, password: Option<U>) -> Result<Self>
    where
        T: AsRef<str>,
        U: AsRef<str>,
    {
        let mut token = INVALID_HANDLE_VALUE;
        let pwd = match password {
            Some(p) => to_utf16(p.as_ref()),
            None => to_utf16(""),
        };

        unsafe {
            cvt(LogonUserW(
                /*lpUsername=*/ to_utf16(user.as_ref()).as_ptr(),
                /*lpDomain=*/ to_utf16(".").as_ptr(),
                /*lpPassword=*/ pwd.as_ptr(),
                /*dwLogonType=*/ LOGON32_LOGON_INTERACTIVE,
                /*dwLogonProvider=*/ LOGON32_PROVIDER_DEFAULT,
                /*phToken=*/ &mut token,
            ))?;

            // Create separate desktop and window station for this user account, so it can get access to them.
            // Otherwise, window applications may crash since they don't have access to current desktop\winstation.
            let new_winsta = cvt(CreateWindowStationW(
                /*lpwinsta=*/ ptr::null(),
                /*dwFlags=*/ 0,
                /*dwDesiredAccess=*/ WINSTA_ALL_ACCESS,
                /*lpsa=*/ ptr::null_mut(),
            ))?;

            let old_winsta = cvt(GetProcessWindowStation())?;
            cvt(SetProcessWindowStation(new_winsta))?;
            let desktop_name = "desktop";
            let desktop = CreateDesktopW(
                /*lpszDesktop=*/ to_utf16(desktop_name).as_ptr(),
                /*lpszDevice=*/ ptr::null(),
                /*pDevmode=*/ ptr::null_mut(),
                /*dwFlags=*/ 0,
                /*dwDesiredAccess=*/ DESKTOP_ALL,
                /*lpsa=*/ ptr::null_mut(),
            );
            cvt(SetProcessWindowStation(old_winsta))?;
            cvt(desktop)?;

            let mut winsta_name_bytes = 0;
            let mut winsta_name_buf = [0 as WCHAR; 128];
            cvt(GetUserObjectInformationW(
                /*hObj=*/ mem::transmute(new_winsta),
                /*nIndex=*/ UOI_NAME as i32,
                /*pvInfo=*/ mem::transmute(winsta_name_buf.as_mut_ptr()),
                /*nLength=*/ (mem::size_of::<WCHAR>() * winsta_name_buf.len()) as DWORD,
                /*lpnLengthNeeded=*/ &mut winsta_name_bytes,
            ))?;

            let winsta_name_len = winsta_name_bytes as usize / mem::size_of::<WCHAR>() - 1;
            let winsta_name = &winsta_name_buf[..winsta_name_len];

            Ok(Self {
                token: Handle(token),
                winsta: new_winsta,
                desktop: desktop,
                desktop_name: to_utf16(format!(
                    "{}\\{}",
                    String::from_utf16(winsta_name).map_err(|e| Error::from(e.to_string()))?,
                    desktop_name
                )),
            })
        }
    }
}

impl Drop for User {
    fn drop(&mut self) {
        unsafe {
            CloseDesktop(self.desktop);
            CloseWindowStation(self.winsta);
        }
    }
}

impl<'a> UserContext<'a> {
    pub fn enter(user: &'a Option<User>) -> Result<Self> {
        if let Some(u) = user {
            unsafe {
                cvt(ImpersonateLoggedOnUser(u.token.0))?;
            }
        }
        Ok(Self(user))
    }
}

impl<'a> Drop for UserContext<'a> {
    fn drop(&mut self) {
        if self.0.is_some() {
            unsafe {
                RevertToSelf();
            }
        }
    }
}

impl EnvBlock {
    pub fn create(user: &Option<User>) -> Result<Self> {
        // https://docs.microsoft.com/en-us/windows/desktop/api/processthreadsapi/nf-processthreadsapi-createprocessa
        //
        // An environment block consists of a null-terminated block of null-terminated strings.
        // Each string is in the following form:
        //     name=value\0
        //
        // A Unicode environment block is terminated by four zero bytes: two for the last string,
        // two more to terminate the block.
        let mut block: *mut u16 = ptr::null_mut();
        let mut len = 0;
        unsafe {
            cvt(CreateEnvironmentBlock(
                mem::transmute(&mut block),
                match user {
                    Some(u) => u.token.0,
                    None => ptr::null_mut(),
                },
                FALSE,
            ))?;

            while !(*block.offset(len) == 0 && *block.offset(len + 1) == 0) {
                len += 1;
            }
        }

        Ok(Self {
            block: block,
            len: len as usize,
        })
    }

    pub fn as_slice(&self) -> &[u16] {
        unsafe { std::slice::from_raw_parts(self.block, self.len) }
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        self.as_slice()
            .split(|c| *c == 0)
            .map(String::from_utf16_lossy)
    }
}

impl Drop for EnvBlock {
    fn drop(&mut self) {
        unsafe {
            DestroyEnvironmentBlock(mem::transmute(self.block));
        }
    }
}

impl StartupInfo {
    pub fn create(
        stdio: &RawStdio,
        inherited_handles: &mut [HANDLE],
        desktop_name: Option<&mut Vec<u16>>,
        show_window: bool,
    ) -> Result<Self> {
        // Unfortunately, winapi-rs does not define this.
        const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: DWORD_PTR = 131074;

        let mut att_list = AttList::allocate(1)?;
        unsafe {
            att_list.update(
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                mem::transmute(inherited_handles.as_mut_ptr()),
                inherited_handles.len() * mem::size_of::<HANDLE>(),
            )?;
        }

        let mut info: STARTUPINFOEXW = unsafe { mem::zeroed() };
        info.lpAttributeList = att_list.ptr;
        info.StartupInfo.cb = mem::size_of_val(&info) as DWORD;
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        info.StartupInfo.wShowWindow = if show_window { SW_SHOW } else { SW_HIDE } as WORD;
        info.StartupInfo.hStdInput = stdio.stdin.0;
        info.StartupInfo.hStdOutput = stdio.stdout.0;
        info.StartupInfo.hStdError = stdio.stderr.0;
        info.StartupInfo.lpDesktop = desktop_name
            .map(|v| v.as_mut_ptr())
            .unwrap_or(ptr::null_mut());

        Ok(StartupInfo {
            base: info,
            _att_list: att_list,
        })
    }
}

impl AttList {
    fn allocate(attribs_count: DWORD) -> Result<Self> {
        unsafe {
            let mut len: SIZE_T = 0;
            InitializeProcThreadAttributeList(ptr::null_mut(), attribs_count, 0, &mut len);
            let ptr: *mut PROC_THREAD_ATTRIBUTE_LIST =
                mem::transmute(alloc_zeroed(Layout::from_size_align_unchecked(len, 4)));

            if ptr.is_null() {
                return Err(Error::from(
                    "Cannot allocate memory for PROC_THREAD_ATTRIBUTE_LIST",
                ));
            }

            cvt(InitializeProcThreadAttributeList(
                /*lpAttributeList=*/ ptr,
                /*dwAttributeCount=*/ attribs_count,
                /*dwFlags=*/ 0,
                /*lpSize=*/ &mut len,
            ))?;

            Ok(Self { ptr: ptr, len: len })
        }
    }

    fn update(&mut self, attribute: DWORD_PTR, value: PVOID, size: SIZE_T) -> Result<()> {
        unsafe {
            cvt(UpdateProcThreadAttribute(
                /*lpAttributeList=*/ self.ptr,
                /*dwFlags=*/ 0,
                /*Attribute=*/ attribute,
                /*lpValue=*/ value,
                /*cbSize=*/ size,
                /*lpPreviousValue=*/ ptr::null_mut(),
                /*lpReturnSize=*/ ptr::null_mut(),
            ))?;
        }
        Ok(())
    }
}

impl Drop for AttList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.ptr);
            dealloc(
                mem::transmute(self.ptr),
                Layout::from_size_align_unchecked(self.len, 4),
            );
        }
    }
}