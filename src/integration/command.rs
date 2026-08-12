use std::io;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

pub(crate) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(crate) fn hook_command(hook_path: &Path, action: Option<&str>) -> String {
    let path = hook_path.display().to_string();
    #[cfg(windows)]
    {
        let mut command = format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File {}",
            windows_command_quote(&path)
        );
        if let Some(action) = action {
            command.push(' ');
            command.push_str(action);
        }
        command
    }

    #[cfg(not(windows))]
    {
        let mut command = format!("bash {}", shell_single_quote(&path));
        if let Some(action) = action {
            command.push(' ');
            command.push_str(action);
        }
        command
    }
}

pub(crate) fn trusted_hook_command(hook_path: &Path, action: Option<&str>) -> io::Result<String> {
    #[cfg(windows)]
    {
        let powershell = system_windows_directory()?
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if !powershell.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "trusted Windows PowerShell executable not found at {}",
                    powershell.display()
                ),
            ));
        }

        let mut command = format!(
            "{} -NoProfile -ExecutionPolicy Bypass -File {}",
            windows_command_quote(&powershell.display().to_string()),
            windows_command_quote(&hook_path.display().to_string())
        );
        if let Some(action) = action {
            command.push(' ');
            command.push_str(action);
        }
        Ok(command)
    }

    #[cfg(not(windows))]
    {
        Ok(hook_command(hook_path, action))
    }
}

#[cfg(windows)]
fn system_windows_directory() -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

    let mut buffer = vec![0_u16; 260];
    loop {
        let length =
            unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = length as usize;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(length + 1, 0);
    }
}

pub(crate) fn legacy_bash_hook_command(hook_path: &Path, action: Option<&str>) -> String {
    let mut command = format!(
        "bash {}",
        shell_single_quote(&hook_path.display().to_string())
    );
    if let Some(action) = action {
        command.push(' ');
        command.push_str(action);
    }
    command
}

#[cfg(windows)]
fn windows_command_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}
