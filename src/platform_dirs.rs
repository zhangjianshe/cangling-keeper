use std::path::PathBuf;

/// Return a per-user application data directory without using WinRT or the
/// newer Known Folder APIs. Environment variables used here are available on
/// Windows 7 and avoid linking the executable to Windows 8's combase.dll.
pub(crate) fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env_path("LOCALAPPDATA").or_else(|| env_path("APPDATA"))
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|home| home.join("Library").join("Application Support"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        env_path("XDG_DATA_HOME").or_else(|| home_dir().map(|home| home.join(".local/share")))
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        home_dir()
    }
}

pub(crate) fn desktop_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        home_dir().map(|home| home.join("Desktop"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        home_dir().map(|home| home.join("Desktop"))
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env_path("USERPROFILE").or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            let mut home = PathBuf::from(drive);
            home.push(path);
            Some(home)
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        env_path("HOME")
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolves_current_platform_directories() {
        assert!(super::data_dir().is_some());
        assert!(super::desktop_dir().is_some());
    }
}
