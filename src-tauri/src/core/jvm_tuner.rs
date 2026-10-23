//! Smart JVM tuning engine.
//!
//! Generates GC-oriented JVM flags from the final heap size, the detected Java
//! major version, the CPU core count and the JVM vendor. This module is pure:
//! no IO, no process spawning - the launch pipeline in `core::launcher` simply
//! consumes `generate_tuned_args` + `merge_args`, which keeps the strategy
//! fully unit-testable.
//!
//! Strategy matrix (thresholds are auditable constants below):
//!
//! | Heap              | Java          | Profile            |
//! | ----------------- | ------------- | ------------------ |
//! | > 8 GB            | >= 21 HotSpot | Generational ZGC   |
//! | > 4 GB            | any HotSpot   | G1Large (tuned)    |
//! | <= 4 GB           | any HotSpot   | G1Small (conservative) |
//! | any               | OpenJ9        | Skipped (HotSpot GC flags do not apply) |

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// How an instance resolves the tuner switch (decision D1: global default +
/// per-instance override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum JvmTunerMode {
    /// Follow the global `enable_jvm_tuner` launcher setting.
    #[default]
    FollowGlobal,
    /// Force tuning on for this instance.
    On,
    /// Force tuning off for this instance.
    Off,
}

impl JvmTunerMode {
    /// Resolves the effective switch for an instance.
    pub fn resolves_to(self, global_enabled: bool) -> bool {
        match self {
            JvmTunerMode::On => true,
            JvmTunerMode::Off => false,
            JvmTunerMode::FollowGlobal => global_enabled,
        }
    }
}

/// The GC strategy selected for a given launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcProfile {
    /// Generational ZGC (heap > 8 GB, HotSpot Java 21+).
    GenerationalZgc,
    /// Tuned G1 for larger heaps (> 4 GB, or ZGC unavailable).
    G1Large,
    /// Conservative G1 for small heaps (<= 4 GB).
    G1Small,
    /// Tuning skipped (OpenJ9 - HotSpot GC flags do not apply there).
    Skipped,
}

/// Inputs gathered by the launch pipeline (or approximated by the preview).
#[derive(Debug, Clone, Copy)]
pub struct TuningInput {
    /// Final heap size in MB (after instance / global / recommended resolution).
    pub max_memory_mb: u32,
    /// Detected Java major version (0 when unknown - the tuner then stays
    /// conservative and avoids version-gated flags).
    pub java_major: u32,
    /// Whether the selected JVM is OpenJ9.
    pub is_openj9: bool,
    /// Logical CPU cores.
    pub cpu_cores: u32,
}

/// The tuning outcome handed to the launch pipeline (or the preview UI).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningResult {
    pub profile: GcProfile,
    /// Ordered GC flags. Contains no `-Xmx`/`-Xms`, no classpath - those stay
    /// owned by the existing pipeline.
    pub args: Vec<String>,
}

// ---------------------------------------------------------------------------
// Strategy thresholds - kept in one place so they are easy to audit.
// ---------------------------------------------------------------------------

/// Above this heap (exclusive) ZGC is eligible on Java 21+.
const ZGC_HEAP_THRESHOLD_MB: u32 = 8192;
/// Generational ZGC requires JEP 439 (Java 21).
const ZGC_MIN_JAVA_MAJOR: u32 = 21;
/// Above this heap (exclusive) the tuned G1 profile is used.
const G1_LARGE_HEAP_THRESHOLD_MB: u32 = 4096;
/// G1 extension flags (NewSize percent bounds, string dedup) are only passed
/// on Java 11+ so Java 8 installs never see an unknown-flag error.
const MIN_JAVA_MAJOR_FOR_ADVANCED_G1_FLAGS: u32 = 11;

/// Logical cores usable by GC workers. Mirrors the widely-used Aikar
/// heuristic: use every core below 4, then half of them, capped at 16.
fn parallel_gc_threads(cpu_cores: u32) -> u32 {
    if cpu_cores < 4 {
        cpu_cores.max(1)
    } else {
        (cpu_cores / 2).clamp(4, 16)
    }
}

/// Generates the tuned GC flag set for the given input.
pub fn generate_tuned_args(input: &TuningInput) -> TuningResult {
    // OpenJ9 does not implement HotSpot GC flags; injecting them is unsafe.
    if input.is_openj9 {
        return TuningResult {
            profile: GcProfile::Skipped,
            args: Vec::new(),
        };
    }

    let cores = parallel_gc_threads(input.cpu_cores);

    // Generational ZGC: large heap on HotSpot Java 21+.
    if input.max_memory_mb > ZGC_HEAP_THRESHOLD_MB && input.java_major >= ZGC_MIN_JAVA_MAJOR {
        let mut args = vec!["-XX:+UseZGC".to_string()];
        // JEP 439 (Java 21/22): opt in to generational mode. JEP 474 (Java 23)
        // made generational the default, so the flag is only needed below 23.
        if input.java_major < 23 {
            args.push("-XX:+ZGenerational".to_string());
        }
        // Soft target so concurrent cycles start before the heap fills up.
        let soft_max = input.max_memory_mb * 3 / 4;
        args.push(format!("-XX:SoftMaxHeapSize={}M", soft_max));
        args.push("-XX:+ExplicitGCInvokesConcurrent".to_string());
        return TuningResult {
            profile: GcProfile::GenerationalZgc,
            args,
        };
    }

    // Tuned G1 for mid/large heaps (or when ZGC is unavailable).
    if input.max_memory_mb > G1_LARGE_HEAP_THRESHOLD_MB {
        let mut args = vec![
            "-XX:+UseG1GC".to_string(),
            "-XX:MaxGCPauseMillis=37".to_string(),
            format!("-XX:ParallelGCThreads={}", cores),
            "-XX:G1HeapRegionSize=16M".to_string(),
            "-XX:G1ReservePercent=20".to_string(),
            "-XX:+ParallelRefProcEnabled".to_string(),
            "-XX:+ExplicitGCInvokesConcurrent".to_string(),
        ];
        if input.java_major >= MIN_JAVA_MAJOR_FOR_ADVANCED_G1_FLAGS {
            args.push("-XX:+UnlockExperimentalVMOptions".to_string());
            args.push("-XX:G1NewSizePercent=28".to_string());
            args.push("-XX:G1MaxNewSizePercent=40".to_string());
            args.push("-XX:+UseStringDeduplication".to_string());
        }
        return TuningResult {
            profile: GcProfile::G1Large,
            args,
        };
    }

    // Conservative G1 for small heaps - only flags supported since Java 8.
    let args = vec![
        "-XX:+UseG1GC".to_string(),
        "-XX:MaxGCPauseMillis=50".to_string(),
        format!("-XX:ParallelGCThreads={}", cores),
        "-XX:+ParallelRefProcEnabled".to_string(),
        "-XX:+ExplicitGCInvokesConcurrent".to_string(),
    ];
    TuningResult {
        profile: GcProfile::G1Small,
        args,
    }
}

/// Parses a `-Xmx` argument (e.g. `-Xmx4G`, `-Xmx4096M`, `-Xmx4194304K`) into megabytes.
pub fn parse_xmx_mb(arg: &str) -> Option<u32> {
    let rest = arg.trim().strip_prefix("-Xmx")?;
    if rest.is_empty() {
        return None;
    }
    let (num_str, unit) = match rest.chars().last()? {
        'g' | 'G' => (&rest[..rest.len() - 1], 'g'),
        'm' | 'M' => (&rest[..rest.len() - 1], 'm'),
        'k' | 'K' => (&rest[..rest.len() - 1], 'k'),
        c if c.is_ascii_digit() => (rest, 'b'),
        _ => return None,
    };
    let value: u64 = num_str.parse().ok()?;
    let mb = match unit {
        'g' => value.saturating_mul(1024),
        'm' => value,
        'k' => value / 1024,
        'b' => value / (1024 * 1024),
        _ => return None,
    };
    if mb == 0 {
        return None;
    }
    Some((mb.min(u32::MAX as u64)) as u32)
}

/// Resolves the effective max heap size in MB, giving the last valid `-Xmx`
/// in `user_extra` precedence over `configured_mb`.
pub fn resolve_effective_max_memory_mb(configured_mb: u32, user_extra: &[String]) -> u32 {
    user_extra
        .iter()
        .rev()
        .find_map(|arg| parse_xmx_mb(arg))
        .unwrap_or(configured_mb)
}

/// Extracts a normalized flag key from a JVM argument so higher-precedence
/// layers can shadow matching base/tuned flags:
/// - `-XX:+UseZGC` / `-XX:-UseG1GC` / `-XX:MaxGCPauseMillis=37` -> `UseZGC` / `UseG1GC` / `MaxGCPauseMillis`
/// - `-Dfoo.bar=baz` -> `foo.bar`
/// - `-Xmx4G` / `-Xms1G` / `-Xmn512M` / `-Xss1M` -> `-Xmx` / `-Xms` / `-Xmn` / `-Xss`
fn flag_key(arg: &str) -> Option<&str> {
    if let Some(rest) = arg.strip_prefix("-XX:") {
        if let Some(key) = rest.strip_prefix('+').or_else(|| rest.strip_prefix('-')) {
            return Some(key);
        }
        return rest.split('=').next();
    }
    if let Some(rest) = arg.strip_prefix("-D") {
        let key = rest.split('=').next()?;
        if !key.is_empty() {
            return Some(key);
        }
    }
    ["-Xmx", "-Xms", "-Xmn", "-Xss"]
        .into_iter()
        .find(|&prefix| arg.starts_with(prefix) && arg.len() > prefix.len())
}

const GC_SELECTOR_KEYS: &[&str] = &[
    "UseZGC",
    "UseG1GC",
    "UseParallelGC",
    "UseParallelOldGC",
    "UseConcMarkSweepGC",
    "UseSerialGC",
    "UseShenandoahGC",
    "UseEpsilonGC",
];

const ZGC_SPECIFIC_KEYS: &[&str] = &[
    "ZGenerational",
    "SoftMaxHeapSize",
    "ZAllocationSpikeTolerance",
    "ZCollectionInterval",
    "ZProactive",
    "ZUncommit",
    "ZUncommitDelay",
];

const G1_SPECIFIC_KEYS: &[&str] = &[
    "G1NewSizePercent",
    "G1MaxNewSizePercent",
    "G1HeapRegionSize",
    "G1ReservePercent",
    "G1MixedGCCountTarget",
    "G1MixedGCLiveThresholdPercent",
    "G1RSetUpdatingPauseTimePercent",
    "G1SummarizingRSetStatsPeriod",
    "InitiatingHeapOccupancyPercent",
    "UseStringDeduplication",
];

const CMS_SPECIFIC_KEYS: &[&str] = &[
    "CMSIncrementalMode",
    "CMSInitiatingOccupancyFraction",
    "UseCMSInitiatingOccupancyOnly",
    "CMSClassUnloadingEnabled",
    "CMSParallelRemarkEnabled",
];

/// Groups of mutually exclusive flag keys.
const MUTEX_FLAG_GROUPS: &[&[&str]] = &[
    GC_SELECTOR_KEYS,
    &[
        "DisableExplicitGC",
        "ExplicitGCInvokesConcurrent",
    ],
    &[
        "java.net.preferIPv4Stack",
        "java.net.preferIPv6Stack",
    ],
];

fn positive_gc_selector(arg: &str) -> Option<&str> {
    let key = arg.strip_prefix("-XX:+")?;
    if GC_SELECTOR_KEYS.contains(&key) {
        Some(key)
    } else {
        None
    }
}

fn is_tuned_collector_specific_key(key: &str) -> bool {
    GC_SELECTOR_KEYS.contains(&key)
        || ZGC_SPECIFIC_KEYS.contains(&key)
        || G1_SPECIFIC_KEYS.contains(&key)
        || CMS_SPECIFIC_KEYS.contains(&key)
        || key == "MaxGCPauseMillis"
        || key == "UnlockExperimentalVMOptions"
}

fn is_incompatible_with_collector(key: &str, collector: &str) -> bool {
    match collector {
        "UseZGC" => {
            G1_SPECIFIC_KEYS.contains(&key)
                || CMS_SPECIFIC_KEYS.contains(&key)
                || key == "MaxGCPauseMillis"
        }
        "UseG1GC" => ZGC_SPECIFIC_KEYS.contains(&key) || CMS_SPECIFIC_KEYS.contains(&key),
        "UseConcMarkSweepGC" => ZGC_SPECIFIC_KEYS.contains(&key) || G1_SPECIFIC_KEYS.contains(&key),
        _ => {
            ZGC_SPECIFIC_KEYS.contains(&key)
                || G1_SPECIFIC_KEYS.contains(&key)
                || CMS_SPECIFIC_KEYS.contains(&key)
        }
    }
}

/// Keys of all groups touched by `keys` (the choice plus every key it excludes).
fn expanded_group_keys<'a>(keys: &HashSet<&'a str>) -> HashSet<&'a str> {
    keys.iter()
        .flat_map(|k| {
            MUTEX_FLAG_GROUPS
                .iter()
                .find(|g| g.contains(k))
                .map(|g| g.to_vec())
                .unwrap_or_default()
        })
        .collect()
}

/// Merges base (version JSON + memory) args, tuned args and user extras.
///
/// Precedence (highest wins): user extras > tuned args > base args.
pub fn merge_args(base: Vec<String>, tuned: &[String], user_extra: &[String]) -> Vec<String> {
    let user_keys: HashSet<&str> = user_extra.iter().filter_map(|a| flag_key(a)).collect();
    let user_literals: HashSet<&String> = user_extra.iter().collect();
    let user_group_keys = expanded_group_keys(&user_keys);

    let tuned_collector = tuned.iter().find_map(|a| positive_gc_selector(a));
    let user_positive_collector = user_extra
        .iter()
        .rev()
        .find_map(|a| positive_gc_selector(a));
    let user_touches_collector = user_keys.iter().any(|k| GC_SELECTOR_KEYS.contains(k));
    let user_overrides_tuned_collector = match (tuned_collector, user_positive_collector) {
        (Some(tc), Some(uc)) => tc != uc,
        (Some(_), None) => user_touches_collector,
        (None, Some(_)) => true,
        (None, None) => false,
    };

    let user_disables_experimental = user_extra
        .iter()
        .any(|a| a == "-XX:-UnlockExperimentalVMOptions");

    // Filter tuned args first so collector-specific tuned flags are dropped if
    // the user overrides the collector, and individual flags shadowed by the
    // user are removed.
    let effective_tuned: Vec<&String> = tuned
        .iter()
        .filter(|arg| {
            if user_literals.contains(arg) {
                return false;
            }
            let key = flag_key(arg);
            if key.is_some_and(|k| user_keys.contains(k) || user_group_keys.contains(k)) {
                return false;
            }
            if user_overrides_tuned_collector && key.is_some_and(is_tuned_collector_specific_key) {
                return false;
            }
            if user_disables_experimental
                && key.is_some_and(|k| k == "G1NewSizePercent" || k == "G1MaxNewSizePercent")
            {
                return false;
            }
            true
        })
        .collect();

    let active_collector = user_positive_collector.or(if user_overrides_tuned_collector {
        None
    } else {
        tuned_collector
    });

    let effective_keys: HashSet<&str> = effective_tuned
        .iter()
        .filter_map(|a| flag_key(a))
        .chain(user_keys.iter().copied())
        .collect();
    let effective_group_keys = expanded_group_keys(&effective_keys);
    let effective_tuned_literals: HashSet<&str> =
        effective_tuned.iter().map(|s| s.as_str()).collect();

    let mut merged: Vec<String> = Vec::with_capacity(base.len() + tuned.len() + user_extra.len());

    for arg in base {
        let key = flag_key(&arg);
        if let Some(k) = key {
            if user_keys.contains(k) || user_group_keys.contains(k) {
                continue;
            }
            if let Some(collector) = active_collector {
                if is_incompatible_with_collector(k, collector) {
                    continue;
                }
            }
            if user_disables_experimental && (k == "G1NewSizePercent" || k == "G1MaxNewSizePercent")
            {
                continue;
            }
            // Shadow base flag if effective_tuned touches the same key or mutex
            // group, unless base has the exact same literal flag (such as
            // `-XX:+UnlockExperimentalVMOptions`), in which case keeping it in
            // place preserves ordering for earlier flags and `effective_tuned`
            // deduplicates it below.
            if (effective_keys.contains(k) || effective_group_keys.contains(k))
                && !effective_tuned_literals.contains(arg.as_str())
            {
                continue;
            }
        }
        merged.push(arg);
    }

    for arg in effective_tuned {
        if !merged.contains(arg) {
            merged.push(arg.clone());
        }
    }

    for arg in user_extra {
        if !merged.contains(arg) {
            merged.push(arg.clone());
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(mem: u32, java: u32) -> TuningInput {
        TuningInput {
            max_memory_mb: mem,
            java_major: java,
            is_openj9: false,
            cpu_cores: 8,
        }
    }

    // -----------------------------------------------------------------------
    // Strategy matrix
    // -----------------------------------------------------------------------

    #[test]
    fn zgc_selected_for_large_heap_on_java21() {
        let r = generate_tuned_args(&input(12288, 21));
        assert_eq!(r.profile, GcProfile::GenerationalZgc);
        assert!(r.args.contains(&"-XX:+UseZGC".to_string()));
        assert!(r.args.contains(&"-XX:+ZGenerational".to_string()));
        assert!(r
            .args
            .contains(&"-XX:SoftMaxHeapSize=9216M".to_string())); // 12288 * 3/4
    }

    #[test]
    fn zgc_omits_generational_flag_on_java23plus() {
        // JEP 474: generational mode is the default from Java 23 on.
        let r = generate_tuned_args(&input(16384, 23));
        assert_eq!(r.profile, GcProfile::GenerationalZgc);
        assert!(!r.args.iter().any(|a| a.contains("ZGenerational")));
    }

    #[test]
    fn zgc_not_selected_below_java21() {
        let r = generate_tuned_args(&input(12288, 17));
        assert_eq!(r.profile, GcProfile::G1Large);
        assert!(r.args.iter().all(|a| !a.contains("UseZGC")));
    }

    #[test]
    fn zgc_heap_threshold_is_exclusive() {
        // Exactly 8 GB stays in the G1 bucket (proposal: "> 8 GB").
        assert_eq!(
            generate_tuned_args(&input(8192, 25)).profile,
            GcProfile::G1Large
        );
        assert_eq!(
            generate_tuned_args(&input(8193, 21)).profile,
            GcProfile::GenerationalZgc
        );
    }

    #[test]
    fn g1_large_for_mid_heaps() {
        let r = generate_tuned_args(&input(6144, 17));
        assert_eq!(r.profile, GcProfile::G1Large);
        assert!(r.args.contains(&"-XX:MaxGCPauseMillis=37".to_string()));
        assert!(r.args.contains(&"-XX:+UnlockExperimentalVMOptions".to_string()));
        assert!(r.args.contains(&"-XX:G1NewSizePercent=28".to_string()));
        assert!(r.args.contains(&"-XX:G1HeapRegionSize=16M".to_string()));
        assert!(r.args.contains(&"-XX:ParallelGCThreads=4".to_string())); // 8 cores / 2
        let unlock_pos = r
            .args
            .iter()
            .position(|a| a == "-XX:+UnlockExperimentalVMOptions")
            .unwrap();
        let new_size_pos = r
            .args
            .iter()
            .position(|a| a == "-XX:G1NewSizePercent=28")
            .unwrap();
        assert!(unlock_pos < new_size_pos);
    }

    #[test]
    fn g1_small_for_small_heaps() {
        let r = generate_tuned_args(&input(4096, 21));
        assert_eq!(r.profile, GcProfile::G1Small);
        assert!(r.args.contains(&"-XX:MaxGCPauseMillis=50".to_string()));
        // Region size / NewSize percent are intentionally absent on small heaps.
        assert!(r.args.iter().all(|a| !a.contains("G1HeapRegionSize")));
        assert!(r.args.iter().all(|a| !a.contains("G1NewSizePercent")));
    }

    #[test]
    fn g1_advanced_flags_skipped_on_java8() {
        let r = generate_tuned_args(&input(6144, 8));
        assert_eq!(r.profile, GcProfile::G1Large);
        assert!(r.args.iter().all(|a| !a.contains("G1NewSizePercent")));
        assert!(r.args.iter().all(|a| !a.contains("StringDeduplication")));
        // Base G1 flags still apply on Java 8.
        assert!(r.args.contains(&"-XX:+UseG1GC".to_string()));
        assert!(r.args.contains(&"-XX:MaxGCPauseMillis=37".to_string()));
    }

    #[test]
    fn unknown_java_version_stays_conservative() {
        // java_major = 0 (preview without a cached scan) must never emit
        // version-gated flags.
        let r = generate_tuned_args(&input(12288, 0));
        assert_eq!(r.profile, GcProfile::G1Large);
        assert!(r.args.iter().all(|a| !a.contains("UseZGC")));
        assert!(r.args.iter().all(|a| !a.contains("G1NewSizePercent")));
    }

    #[test]
    fn openj9_skips_tuning() {
        let r = generate_tuned_args(&TuningInput {
            max_memory_mb: 12288,
            java_major: 21,
            is_openj9: true,
            cpu_cores: 8,
        });
        assert_eq!(r.profile, GcProfile::Skipped);
        assert!(r.args.is_empty());
    }

    // -----------------------------------------------------------------------
    // CPU scaling
    // -----------------------------------------------------------------------

    #[test]
    fn parallel_threads_scale_with_cores() {
        assert_eq!(parallel_gc_threads(0), 1);
        assert_eq!(parallel_gc_threads(1), 1);
        assert_eq!(parallel_gc_threads(2), 2);
        assert_eq!(parallel_gc_threads(4), 4);
        assert_eq!(parallel_gc_threads(8), 4);
        assert_eq!(parallel_gc_threads(16), 8);
        assert_eq!(parallel_gc_threads(64), 16); // capped
    }

    // -----------------------------------------------------------------------
    // Merge / precedence
    // -----------------------------------------------------------------------

    #[test]
    fn user_extra_overrides_tuned_gc_flags() {
        let tuned = vec![
            "-XX:+UseZGC".to_string(),
            "-XX:MaxGCPauseMillis=37".to_string(),
        ];
        let user = vec![
            "-XX:+UseG1GC".to_string(),
            "-XX:MaxGCPauseMillis=200".to_string(),
        ];
        let merged = merge_args(vec!["-cp".to_string()], &tuned, &user);
        assert!(merged.contains(&"-XX:+UseG1GC".to_string()));
        assert!(merged.contains(&"-XX:MaxGCPauseMillis=200".to_string()));
        assert!(!merged.contains(&"-XX:+UseZGC".to_string()));
        assert!(!merged.contains(&"-XX:MaxGCPauseMillis=37".to_string()));
        assert!(merged.contains(&"-cp".to_string()));
    }

    #[test]
    fn user_negated_flag_shadows_tuned_positive() {
        let tuned = vec!["-XX:+ParallelRefProcEnabled".to_string()];
        let user = vec!["-XX:-ParallelRefProcEnabled".to_string()];
        let merged = merge_args(Vec::new(), &tuned, &user);
        assert_eq!(merged, vec!["-XX:-ParallelRefProcEnabled".to_string()]);
    }

    #[test]
    fn user_gc_selector_clears_the_whole_selector_group() {
        // UseZGC and UseG1GC are different keys but mutually exclusive: once
        // the user picks one, the engine's selector and collector-specific
        // tuned flags must be dropped entirely.
        let tuned = vec![
            "-XX:+UseZGC".to_string(),
            "-XX:+ZGenerational".to_string(),
            "-XX:SoftMaxHeapSize=6144M".to_string(),
            "-XX:+ExplicitGCInvokesConcurrent".to_string(),
        ];
        let user = vec!["-XX:+UseG1GC".to_string()];
        let merged = merge_args(Vec::new(), &tuned, &user);
        assert_eq!(
            merged,
            vec![
                "-XX:+ExplicitGCInvokesConcurrent".to_string(),
                "-XX:+UseG1GC".to_string()
            ]
        );
    }

    #[test]
    fn tuned_zgc_shadows_base_g1_selector_and_flags() {
        let base = vec![
            "-XX:+UseG1GC".to_string(),
            "-XX:+UnlockExperimentalVMOptions".to_string(),
            "-XX:G1NewSizePercent=20".to_string(),
            "-XX:G1ReservePercent=20".to_string(),
            "-XX:MaxGCPauseMillis=50".to_string(),
            "-XX:G1HeapRegionSize=32M".to_string(),
        ];
        let tuned = generate_tuned_args(&input(12288, 21)).args;
        let merged = merge_args(base, &tuned, &[]);
        assert!(merged.contains(&"-XX:+UseZGC".to_string()));
        assert!(!merged.contains(&"-XX:+UseG1GC".to_string()));
        assert!(merged.iter().all(|a| !a.contains("G1")));
        assert!(merged.iter().all(|a| !a.contains("MaxGCPauseMillis")));
    }

    #[test]
    fn resolve_effective_max_memory_parses_user_xmx() {
        assert_eq!(resolve_effective_max_memory_mb(4096, &[]), 4096);
        assert_eq!(
            resolve_effective_max_memory_mb(4096, &["-Xmx12G".to_string()]),
            12288
        );
        assert_eq!(
            resolve_effective_max_memory_mb(
                4096,
                &["-Xmx8G".to_string(), "-Xmx10240m".to_string()]
            ),
            10240
        );
    }

    #[test]
    fn base_flags_shadowed_by_user_with_same_key() {
        let base = vec![
            "-XX:MaxGCPauseMillis=100".to_string(),
            "-Dfoo=bar".to_string(),
        ];
        let user = vec!["-XX:MaxGCPauseMillis=200".to_string()];
        let merged = merge_args(base, &[], &user);
        assert_eq!(
            merged,
            vec!["-Dfoo=bar".to_string(), "-XX:MaxGCPauseMillis=200".to_string()]
        );
    }

    #[test]
    fn non_xx_user_args_keep_literal_dedup() {
        let base = vec!["-Dfoo=bar".to_string()];
        let user = vec!["-Dfoo=bar".to_string(), "-Dfoo2=baz".to_string()];
        let merged = merge_args(base, &[], &user);
        assert_eq!(merged, vec!["-Dfoo=bar".to_string(), "-Dfoo2=baz".to_string()]);
    }

    #[test]
    fn user_system_property_and_memory_flags_shadow_base() {
        let base = vec![
            "-Xms1024M".to_string(),
            "-Xmx4096M".to_string(),
            "-Djava.net.preferIPv4Stack=true".to_string(),
            "-Dfoo=old".to_string(),
        ];
        let user = vec![
            "-Xms512M".to_string(),
            "-Djava.net.preferIPv6Stack=true".to_string(),
            "-Dfoo=new".to_string(),
        ];
        let merged = merge_args(base, &[], &user);
        assert_eq!(
            merged,
            vec![
                "-Xmx4096M".to_string(),
                "-Xms512M".to_string(),
                "-Djava.net.preferIPv6Stack=true".to_string(),
                "-Dfoo=new".to_string(),
            ]
        );
    }

    #[test]
    fn tuned_args_not_duplicated_when_equal_to_base() {
        let base = vec!["-XX:+ExplicitGCInvokesConcurrent".to_string()];
        let tuned = vec![
            "-XX:+ExplicitGCInvokesConcurrent".to_string(),
            "-XX:+UseG1GC".to_string(),
        ];
        let merged = merge_args(base, &tuned, &[]);
        let count = merged
            .iter()
            .filter(|a| a.as_str() == "-XX:+ExplicitGCInvokesConcurrent")
            .count();
        assert_eq!(count, 1);
        assert!(merged.contains(&"-XX:+UseG1GC".to_string()));
    }

    #[test]
    fn empty_tuned_output_matches_legacy_order() {
        let base = vec!["-Xms1G".to_string(), "-Dlog4j=x".to_string()];
        let user = vec!["-XX:+UseG1GC".to_string()];
        let merged = merge_args(base, &[], &user);
        assert_eq!(
            merged,
            vec![
                "-Xms1G".to_string(),
                "-Dlog4j=x".to_string(),
                "-XX:+UseG1GC".to_string()
            ]
        );
    }
}
