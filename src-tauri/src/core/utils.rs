/// Compare two version strings numerically (segment by segment)
/// e.g., "0.9.1" < "0.10.1" < "0.19.1"
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // If both strings are exactly the same, skip further logic
    if a == b {
        return std::cmp::Ordering::Equal;
    }

    let a_parts: Vec<u32> = a
        .split(|c: char| c == '.' || c == '-' || c == '_')
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b
        .split(|c: char| c == '.' || c == '-' || c == '_')
        .filter_map(|s| s.parse().ok())
        .collect();

    // If parsing fails for both, fall back to string comparison
    if a_parts.is_empty() && b_parts.is_empty() {
        return a.cmp(b);
    }

    for (a_part, b_part) in a_parts.iter().zip(b_parts.iter()) {
        match a_part.cmp(b_part) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    // If all compared parts are equal, shorter version comes first
    a_parts.len().cmp(&b_parts.len())
}

/// Create a tokio Command that hides the console window on Windows.
pub fn create_hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    let mut std_cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(0x08000000);
    }
    tokio::process::Command::from(std_cmd)
}

/// Create a std Command that hides the console window on Windows.
pub fn create_hidden_std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut std_cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(0x08000000);
    }
    std_cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("1.0", "1.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("1.0.0", "1.0.1"), std::cmp::Ordering::Less);
        assert_eq!(compare_versions("1.10.1", "1.9.5"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("2.0-beta", "2.0-alpha"), std::cmp::Ordering::Equal); // Note: Current logic drops non-numbers
        assert_eq!(compare_versions("1.2", "1.2.3"), std::cmp::Ordering::Less);
    }
}
