/// Compare two version strings numerically (segment by segment)
/// e.g., "0.9.1" < "0.10.1" < "0.19.1"
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // If both strings are exactly the same, skip further logic
    if a == b {
        return std::cmp::Ordering::Equal;
    }

    let a_parts: Vec<u32> = a.split(|c: char| c == '.' || c == '-' || c == '_')
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b.split(|c: char| c == '.' || c == '-' || c == '_')
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
