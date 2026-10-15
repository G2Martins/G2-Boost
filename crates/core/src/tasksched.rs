//! Task Scheduler through COM (`ITaskService`), never `schtasks.exe`.
//! Two uses: run Windows' own `\Microsoft\Windows\Servicing\StartComponentCleanup`
//! (DISM component cleanup without `DISM.exe`), and the "start with Windows" logon task.

use std::path::Path;

use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, IRegisteredTask, ITaskFolder, ITaskService, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN, TASK_RUNLEVEL_HIGHEST,
    TASK_RUNLEVEL_LUA, TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HRESULT, Interface};

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("task_not_found")]
    NotFound,
    #[error("com: {0}")]
    Com(String),
}

const E_ACCESS_DENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);
const E_FILE_NOT_FOUND: HRESULT = HRESULT(0x8007_0002_u32 as i32);
const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x8001_0106_u32 as i32);

fn map(e: windows::core::Error) -> TaskError {
    match e.code() {
        E_ACCESS_DENIED => TaskError::NeedsAdmin,
        E_FILE_NOT_FOUND => TaskError::NotFound,
        code => TaskError::Com(format!("{code:?}: {}", e.message())),
    }
}

/// COM apartment for the current thread; uninitialised on drop unless the thread already
/// had COM in another mode (then we just borrow it). Shared with the WMI client in `restore`.
pub(crate) struct Com {
    owned: bool,
}

impl Com {
    pub(crate) fn init() -> Result<Self, TaskError> {
        // SAFETY: plain COM initialisation on this thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        hr.ok().map_err(map)?;
        Ok(Self { owned: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balanced with the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    }
}

/// `DOMAIN\user` of the interactive user (what the logon trigger wants).
fn current_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
        _ => user,
    }
}

fn service() -> Result<ITaskService, TaskError> {
    // SAFETY: standard in-proc activation; Connect with empty VARIANTs = local machine, current user.
    unsafe {
        let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(map)?;
        let none = VARIANT::default();
        svc.Connect(&none, &none, &none, &none).map_err(map)?;
        Ok(svc)
    }
}

fn folder(svc: &ITaskService, path: &str) -> Result<ITaskFolder, TaskError> {
    // SAFETY: valid service; BSTR path.
    unsafe { svc.GetFolder(&BSTR::from(path)) }.map_err(map)
}

/// Does `\folder\name` exist? (read-only, no admin)
pub fn task_exists(folder_path: &str, name: &str) -> bool {
    let Ok(_com) = Com::init() else {
        return false;
    };
    let Ok(svc) = service() else {
        return false;
    };
    folder(&svc, folder_path)
        .and_then(|f| unsafe { f.GetTask(&BSTR::from(name)) }.map_err(map))
        .is_ok()
}

/// Enabled flag of `\folder\name`; `Ok(None)` when the folder or task does not exist.
pub fn task_enabled(folder_path: &str, name: &str) -> Result<Option<bool>, TaskError> {
    let _com = Com::init()?;
    let svc = service()?;
    let f = match folder(&svc, folder_path) {
        Ok(f) => f,
        Err(TaskError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    // SAFETY: valid folder; BSTR name.
    match unsafe { f.GetTask(&BSTR::from(name)) } {
        Ok(task) => Ok(Some(unsafe { task.Enabled() }.map_err(map)?.as_bool())),
        Err(e) if e.code() == E_FILE_NOT_FOUND => Ok(None),
        Err(e) => Err(map(e)),
    }
}

/// Enable/disable an existing task (admin for Microsoft's tasks). Missing task = no-op.
pub fn set_task_enabled(folder_path: &str, name: &str, enabled: bool) -> Result<(), TaskError> {
    let _com = Com::init()?;
    let svc = service()?;
    let f = match folder(&svc, folder_path) {
        Ok(f) => f,
        Err(TaskError::NotFound) => return Ok(()),
        Err(e) => return Err(e),
    };
    // SAFETY: valid folder/task; VARIANT_BOOL flag.
    match unsafe { f.GetTask(&BSTR::from(name)) } {
        Ok(task) => unsafe { task.SetEnabled(if enabled { VARIANT_TRUE } else { VARIANT_FALSE }) }.map_err(map),
        Err(e) if e.code() == E_FILE_NOT_FOUND => Ok(()),
        Err(e) => Err(map(e)),
    }
}

/// Start an existing task (e.g. Windows' StartComponentCleanup) and return immediately.
pub fn run_task(folder_path: &str, name: &str) -> Result<(), TaskError> {
    let _com = Com::init()?;
    let svc = service()?;
    let f = folder(&svc, folder_path)?;
    // SAFETY: valid folder; empty VARIANT = no parameters.
    unsafe {
        let task: IRegisteredTask = f.GetTask(&BSTR::from(name)).map_err(map)?;
        task.Run(&VARIANT::default()).map_err(map)?;
    }
    Ok(())
}

/// Register (or replace) a task in the root folder that runs `exe args` at the current user's
/// logon, after `delay_secs`. `highest` = "run with highest privileges" (needs admin to register).
pub fn register_logon_task(name: &str, exe: &Path, args: &str, delay_secs: u32, highest: bool) -> Result<(), TaskError> {
    let _com = Com::init()?;
    let svc = service()?;
    // SAFETY: every interface comes from the live service; BSTR/VARIANT values are owned here.
    unsafe {
        let def = svc.NewTask(0).map_err(map)?;
        let reg = def.RegistrationInfo().map_err(map)?;
        reg.SetAuthor(&BSTR::from("G2 Boost")).map_err(map)?;
        reg.SetDescription(&BSTR::from("G2 Boost: start with Windows (tray, routines)")).map_err(map)?;

        let principal = def.Principal().map_err(map)?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN).map_err(map)?;
        principal
            .SetRunLevel(if highest { TASK_RUNLEVEL_HIGHEST } else { TASK_RUNLEVEL_LUA })
            .map_err(map)?;

        let settings = def.Settings().map_err(map)?;
        settings.SetStartWhenAvailable(VARIANT_TRUE).map_err(map)?;
        settings.SetDisallowStartIfOnBatteries(VARIANT_FALSE).map_err(map)?;
        settings.SetStopIfGoingOnBatteries(VARIANT_FALSE).map_err(map)?;
        settings.SetExecutionTimeLimit(&BSTR::from("PT0S")).map_err(map)?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW).map_err(map)?;

        let triggers = def.Triggers().map_err(map)?;
        let trigger = triggers.Create(TASK_TRIGGER_LOGON).map_err(map)?;
        let logon: ILogonTrigger = trigger.cast().map_err(map)?;
        // Scoped to this user: an "any user" logon trigger needs admin to register.
        logon.SetUserId(&BSTR::from(current_user())).map_err(map)?;
        logon.SetDelay(&BSTR::from(format!("PT{delay_secs}S"))).map_err(map)?;

        let actions = def.Actions().map_err(map)?;
        let action = actions.Create(TASK_ACTION_EXEC).map_err(map)?;
        let exec: IExecAction = action.cast().map_err(map)?;
        exec.SetPath(&BSTR::from(exe.to_string_lossy().as_ref())).map_err(map)?;
        exec.SetArguments(&BSTR::from(args)).map_err(map)?;

        let root = folder(&svc, "\\")?;
        let none = VARIANT::default();
        root.RegisterTaskDefinition(
            &BSTR::from(name),
            &def,
            TASK_CREATE_OR_UPDATE.0,
            &none,
            &none,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &none,
        )
        .map_err(map)?;
    }
    Ok(())
}

/// Remove a root-folder task; missing is not an error.
pub fn delete_task(name: &str) -> Result<(), TaskError> {
    let _com = Com::init()?;
    let svc = service()?;
    let root = folder(&svc, "\\")?;
    // SAFETY: valid folder; flags must be 0.
    match unsafe { root.DeleteTask(&BSTR::from(name), 0) }.map_err(map) {
        Err(TaskError::NotFound) | Ok(()) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_component_cleanup_task_is_visible() {
        assert!(task_exists("\\Microsoft\\Windows\\Servicing", "StartComponentCleanup"));
        assert!(!task_exists("\\", "G2 Boost definitely-not-a-task"));
        // The Compatibility Appraiser task exists on every Windows 10/11 (renamed "… Exp" on 24H2+) and is readable without admin.
        let folder = "\\Microsoft\\Windows\\Application Experience";
        assert!(
            task_enabled(folder, "Microsoft Compatibility Appraiser").unwrap().is_some()
                || task_enabled(folder, "Microsoft Compatibility Appraiser Exp").unwrap().is_some()
        );
        assert_eq!(task_enabled("\\No\\Such\\Folder", "x").unwrap(), None);
        assert_eq!(task_enabled("\\", "G2 Boost definitely-not-a-task").unwrap(), None);
    }

    /// Registers a least-privilege task as the current user, then deletes it. Ignored by
    /// default because it writes to the scheduler; run once by hand.
    #[test]
    #[ignore]
    fn logon_task_roundtrip() {
        let exe = std::env::current_exe().unwrap();
        register_logon_task("G2 Boost Test", &exe, "--test", 5, false).unwrap();
        assert!(task_exists("\\", "G2 Boost Test"));
        delete_task("G2 Boost Test").unwrap();
        assert!(!task_exists("\\", "G2 Boost Test"));
        delete_task("G2 Boost Test").unwrap();
    }
}
