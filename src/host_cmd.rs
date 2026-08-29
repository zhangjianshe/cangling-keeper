use std::ffi::OsStr;
use std::process::Command;

/// Spawn a host binary without AppImage/linuxdeploy library search paths.
///
/// AppRun prepends `$APPDIR/usr/lib` to `LD_LIBRARY_PATH` so WebKitGTK can
/// find bundled `.so` files. Child processes inherit that path, so host tools
/// such as `git` load the bundle's older `libnghttp2` against the distro's
/// `libcurl` and crash:
///
/// ```text
/// git-remote-https: symbol lookup error: libcurl-gnutls.so.4:
/// undefined symbol: nghttp2_option_set_no_rfc9113_leading_and_trailing_ws_validation
/// ```
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    apply_host_env(&mut cmd);
    cmd
}

fn apply_host_env(cmd: &mut Command) {
    let Some(appdir) = std::env::var_os("APPDIR") else {
        return;
    };
    let Some(appdir) = appdir.to_str() else {
        return;
    };
    if appdir.is_empty() {
        return;
    }

    // Colon-separated search paths that linuxdeploy / the GTK hook inject.
    for key in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "PATH",
        "PYTHONPATH",
        "PERLLIB",
        "PERL5LIB",
        "GCONV_PATH",
        "QT_PLUGIN_PATH",
        "QT_QPA_PLATFORM_PLUGIN_PATH",
        "GTK_PATH",
        "GIO_MODULE_DIR",
        "GIO_EXTRA_MODULES",
        "GI_TYPELIB_PATH",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "XDG_DATA_DIRS",
    ] {
        match std::env::var(key) {
            Ok(val) => match strip_appdir_entries(&val, appdir) {
                Some(cleaned) => {
                    cmd.env(key, cleaned);
                }
                None => {
                    cmd.env_remove(key);
                }
            },
            Err(_) => {}
        }
    }

    // Single-value vars that linuxdeploy-plugin-gtk.sh points into APPDIR.
    for key in [
        "GTK_DATA_PREFIX",
        "GTK_EXE_PREFIX",
        "GTK_IM_MODULE_FILE",
        "GDK_PIXBUF_MODULE_FILE",
        "GSETTINGS_SCHEMA_DIR",
        "PYTHONHOME",
    ] {
        if let Ok(val) = std::env::var(key) {
            if path_is_under_appdir(&val, appdir) {
                cmd.env_remove(key);
            }
        }
    }
}

fn strip_appdir_entries(value: &str, appdir: &str) -> Option<String> {
    let appdir = appdir.trim_end_matches('/');
    if appdir.is_empty() {
        return Some(value.to_string());
    }
    let kept: Vec<&str> = value
        .split(':')
        .filter(|part| !part.is_empty() && !path_is_under_appdir(part, appdir))
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(kept.join(":"))
    }
}

fn path_is_under_appdir(path: &str, appdir: &str) -> bool {
    let appdir = appdir.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    path == appdir || path.starts_with(&format!("{appdir}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_appdir_lib_paths() {
        let appdir = "/tmp/.mount_canglXYZ";
        let value =
            format!("{appdir}/usr/lib:{appdir}/usr/lib/x86_64-linux-gnu:/usr/lib:/lib");
        assert_eq!(
            strip_appdir_entries(&value, appdir).as_deref(),
            Some("/usr/lib:/lib")
        );
    }

    #[test]
    fn removes_var_when_only_appdir() {
        let appdir = "/tmp/.mount_canglXYZ";
        let value = format!("{appdir}/usr/lib:{appdir}/usr/lib64");
        assert_eq!(strip_appdir_entries(&value, appdir), None);
    }

    #[test]
    fn leaves_unrelated_paths() {
        assert_eq!(
            strip_appdir_entries("/usr/lib:/lib", "/tmp/.mount_x").as_deref(),
            Some("/usr/lib:/lib")
        );
    }

    #[test]
    fn does_not_strip_similar_prefix() {
        let appdir = "/tmp/.mount_canglXYZ";
        let value = format!("{appdir}_other/lib:/usr/lib");
        assert_eq!(
            strip_appdir_entries(&value, appdir).as_deref(),
            Some(value.as_str())
        );
    }
}
