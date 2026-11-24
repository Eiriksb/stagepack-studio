//! Windows: each program runs in a job of its own, which ends every process in it when the job is
//! closed, so a program can't outlive the helper, and which can be ended whole by force.
//! Not compiled on the machine this was written on: check it on Windows.
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

/// The job a program runs in. Closing it (including when the helper dies) ends the program.
pub struct Group(isize);

impl Group {
    /// Makes the job and puts the just-started program in it. Whatever the program starts after that
    /// is in the job too.
    pub fn of(child: &(dyn portable_pty::Child + Send + Sync)) -> Result<Group, String> {
        let sorry = || "The computer wouldn't keep the program in check, so it wasn't run.".to_string();
        // SAFETY: plain calls into the system with values made here; every handle opened is closed.
        unsafe {
            let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(sorry());
            }
            let group = Group(job as isize);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 {
                return Err(sorry());
            }
            // The program's own handle if portable-pty gives it, else one opened from its id.
            let (process, opened) = match child.as_raw_handle() {
                Some(h) => (h as HANDLE, false),
                None => {
                    let pid = child.process_id().ok_or_else(sorry)?;
                    (OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid), true)
                }
            };
            if process.is_null() {
                return Err(sorry());
            }
            let assigned = AssignProcessToJobObject(job, process);
            if opened {
                CloseHandle(process);
            }
            if assigned == 0 {
                return Err(sorry());
            }
            Ok(group)
        }
    }

    /// Whether any process of the job is still there.
    pub fn exists(&self) -> bool {
        // SAFETY: the handle is ours and open; the buffer is the size the system is told.
        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                self.0 as HANDLE,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            );
            ok != 0 && info.ActiveProcesses > 0
        }
    }

    /// The polite end is closing the pseudo-console, which the caller does by dropping the master.
    pub fn ask_politely(&self) {}

    pub fn force(&self) {
        // SAFETY: the handle is ours and open.
        unsafe {
            TerminateJobObject(self.0 as HANDLE, 1);
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        // SAFETY: closing our own handle; with the kill-on-close limit this ends what is left.
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}
