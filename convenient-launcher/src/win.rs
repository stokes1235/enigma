use std::{ffi::{c_void, OsString}, fs::File, io, mem, os::windows::{ffi::OsStringExt, io::AsRawHandle}, path::{Path, PathBuf}, ptr};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct SystemTimeFields { year:u16, month:u16, day_of_week:u16, day:u16, hour:u16, minute:u16, second:u16, millis:u16 }
#[repr(C)]
struct ProcessEntry {
    size: u32, usage: u32, pid: u32, heap_id: usize, module_id: u32,
    threads: u32, parent_pid: u32, base_priority: i32, flags: u32, name: [u16; 260],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime { low: u32, high: u32 }
#[repr(C)]
struct FileInfo {
    attributes: u32, creation: FileTime, access: FileTime, write: FileTime,
    volume: u32, size_high: u32, size_low: u32, links: u32, index_high: u32, index_low: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct TcpRow { state: u32, local_addr: u32, local_port: u32, remote_addr: u32, remote_port: u32, pid: u32 }

#[link(name="kernel32")]
extern "system" {
    fn GetLocalTime(time: *mut SystemTimeFields);
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(process: Handle, flags: u32, name: *mut u16, size: *mut u32) -> i32;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn TerminateProcess(handle: Handle, exit_code:u32) -> i32;
    fn GetFileInformationByHandle(handle: Handle, info: *mut FileInfo) -> i32;
}
#[link(name="iphlpapi")]
extern "system" {
    fn GetExtendedTcpTable(table: *mut c_void, size: *mut u32, ordered: i32, family: u32, class: u32, reserved: u32) -> u32;
}

struct WinHandle(Handle);
impl Drop for WinHandle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
#[derive(Debug)]
pub struct Process { pub pid: u32, pub parent: u32, pub name: String }

pub fn processes() -> io::Result<Vec<Process>> {
    let raw = unsafe { CreateToolhelp32Snapshot(0x0000_0002, 0) };
    if raw as isize == -1 { return Err(io::Error::last_os_error()); }
    let handle = WinHandle(raw);
    let mut entry: ProcessEntry = unsafe { mem::zeroed() };
    entry.size = mem::size_of::<ProcessEntry>() as u32;
    let mut result = Vec::new();
    if unsafe { Process32FirstW(handle.0, &mut entry) } == 0 { return Err(io::Error::last_os_error()); }
    loop {
        let len = entry.name.iter().position(|c| *c == 0).unwrap_or(entry.name.len());
        result.push(Process { pid: entry.pid, parent: entry.parent_pid, name: OsString::from_wide(&entry.name[..len]).to_string_lossy().into_owned() });
        if unsafe { Process32NextW(handle.0, &mut entry) } == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(18) { return Err(e); } // ERROR_NO_MORE_FILES
            break;
        }
    }
    Ok(result)
}

pub struct WatchedGame { handle: WinHandle, pub pid: u32 }
impl WatchedGame {
    pub fn open(pid: u32, expected_path: &Path) -> io::Result<Self> {
        let raw = unsafe { OpenProcess(0x0010_0000 | 0x1000, 0, pid) };
        if raw.is_null() { return Err(io::Error::last_os_error()); }
        let handle = WinHandle(raw);
        let mut buf = vec![0u16; 32768];
        let mut size = buf.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, buf.as_mut_ptr(), &mut size) } == 0 { return Err(io::Error::last_os_error()); }
        let actual = PathBuf::from(OsString::from_wide(&buf[..size as usize]));
        let actual = std::fs::canonicalize(actual)?;
        let expected = std::fs::canonicalize(expected_path)?;
        if !actual.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("游戏进程路径不匹配: {}", actual.display())));
        }
        Ok(Self { handle, pid })
    }
    pub fn is_running(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(self.handle.0, 0) } {
            0 => Ok(false), 258 => Ok(true), _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn wait(&self) -> io::Result<()> {
        match unsafe { WaitForSingleObject(self.handle.0, u32::MAX) } {
            0 => Ok(()), _ => Err(io::Error::last_os_error()),
        }
    }
}

// A held process handle cannot accidentally refer to a new process if its PID is reused.
pub struct WatchedProcess { handle:WinHandle, pub pid:u32 }
impl WatchedProcess {
    pub fn open(pid:u32, expected_path:Option<&Path>, terminate:bool)->io::Result<Self> {
        let access=0x0010_0000 | 0x1000 | if terminate {1} else {0};
        let raw=unsafe {OpenProcess(access,0,pid)};
        if raw.is_null(){return Err(io::Error::last_os_error());}
        let handle=WinHandle(raw);
        if let Some(expected)=expected_path {
            let mut buf=vec![0u16;32768]; let mut size=buf.len() as u32;
            if unsafe {QueryFullProcessImageNameW(handle.0,0,buf.as_mut_ptr(),&mut size)}==0 {return Err(io::Error::last_os_error());}
            let actual=std::fs::canonicalize(PathBuf::from(OsString::from_wide(&buf[..size as usize])))?;
            let expected=std::fs::canonicalize(expected)?;
            if !actual.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy()) {
                return Err(io::Error::new(io::ErrorKind::InvalidData,format!("进程路径不匹配: {}",actual.display())));
            }
        }
        Ok(Self{handle,pid})
    }
    pub fn is_running(&self)->io::Result<bool> {
        match unsafe {WaitForSingleObject(self.handle.0,0)} {0=>Ok(false),258=>Ok(true),_=>Err(io::Error::last_os_error())}
    }
    pub fn wait(&self)->io::Result<()> {
        match unsafe {WaitForSingleObject(self.handle.0,u32::MAX)} {0=>Ok(()),_=>Err(io::Error::last_os_error())}
    }
    pub fn terminate(&self)->io::Result<()> {
        if !self.is_running()? {return Ok(());}
        if unsafe {TerminateProcess(self.handle.0,1)}==0 {
            let e=io::Error::last_os_error();
            if !self.is_running()? {return Ok(());}
            return Err(e);
        }
        self.wait()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileId { pub volume: u32, pub index_high: u32, pub index_low: u32 }
pub fn file_id(file: &File) -> io::Result<FileId> {
    let mut info: FileInfo = unsafe { mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as Handle, &mut info) } == 0 { return Err(io::Error::last_os_error()); }
    Ok(FileId { volume: info.volume, index_high: info.index_high, index_low: info.index_low })
}

pub fn listeners() -> io::Result<Vec<(u16, u32)>> {
    let mut len = 0u32;
    let status = unsafe { GetExtendedTcpTable(ptr::null_mut(), &mut len, 0, 2, 3, 0) };
    if status != 122 && status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
    // Table may grow between the sizing and retrieval calls.
    for _ in 0..4 {
        let mut buf = vec![0u32; ((len as usize + 3) / 4).max(1)];
        let status = unsafe { GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut len, 0, 2, 3, 0) };
        if status == 122 { continue; }
        if status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
        let count = buf[0] as usize;
        let available = (len as usize).saturating_sub(4) / mem::size_of::<TcpRow>();
        if count > available { return Err(io::Error::new(io::ErrorKind::InvalidData, "TCP 监听表长度异常")); }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let offset = 4 + i * mem::size_of::<TcpRow>();
            let row = unsafe { ptr::read_unaligned((buf.as_ptr() as *const u8).add(offset).cast::<TcpRow>()) };
            out.push((u16::from_be(row.local_port as u16), row.pid));
        }
        return Ok(out);
    }
    Err(io::Error::new(io::ErrorKind::Other, "无法读取稳定的 TCP 监听表"))
}


pub fn local_timestamp() -> String {
    let mut t=SystemTimeFields::default(); unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}",t.year,t.month,t.day,t.hour,t.minute,t.second)
}
