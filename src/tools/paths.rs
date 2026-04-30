/// Get the rtop config directory.
///
/// Priority: `XDG_CONFIG_HOME/rtop` > `%APPDATA%/rtop` (via `directories` crate)
pub fn config_dir() -> std::path::PathBuf {
    config_dir_inner(std::env::var("XDG_CONFIG_HOME").ok().as_deref())
}

fn config_dir_inner(xdg: Option<&str>) -> std::path::PathBuf {
    if let Some(xdg) = xdg {
        let p = std::path::PathBuf::from(xdg);
        if p.is_absolute() {
            return p.join("rtop");
        }
    }
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("rtop"))
        .unwrap_or_else(|| std::path::PathBuf::from("rtop"))
}

/// Get the rtop log/state directory.
///
/// Priority: `XDG_STATE_HOME/rtop` > `%LOCALAPPDATA%/rtop` (via `directories` crate)
pub fn data_dir() -> std::path::PathBuf {
    data_dir_inner(std::env::var("XDG_STATE_HOME").ok().as_deref())
}

fn data_dir_inner(xdg: Option<&str>) -> std::path::PathBuf {
    if let Some(xdg) = xdg {
        let p = std::path::PathBuf::from(xdg);
        if p.is_absolute() {
            return p.join("rtop");
        }
    }
    directories::BaseDirs::new()
        .map(|d| d.data_local_dir().join("rtop"))
        .unwrap_or_else(|| std::path::PathBuf::from("rtop"))
}

#[cfg(test)]
/// Get the system hostname.
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
/// Get the current username.
pub fn username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_returns_path_with_rtop_suffix() {
        let dir = config_dir();
        assert!(
            dir.ends_with("rtop"),
            "expected path ending with 'rtop', got: {:?}",
            dir
        );
    }

    #[test]
    fn data_dir_returns_path_with_rtop_suffix() {
        let dir = data_dir();
        assert!(
            dir.ends_with("rtop"),
            "expected path ending with 'rtop', got: {:?}",
            dir
        );
    }

    #[test]
    fn config_dir_respects_xdg_env() {
        let dir = config_dir_inner(Some("C:\\custom\\xdg"));
        assert_eq!(dir, std::path::PathBuf::from("C:\\custom\\xdg\\rtop"));
    }

    #[test]
    fn config_dir_ignores_relative_xdg() {
        let dir = config_dir_inner(Some("relative/path"));
        assert_ne!(dir, std::path::PathBuf::from("relative/path/rtop"));
    }
}
