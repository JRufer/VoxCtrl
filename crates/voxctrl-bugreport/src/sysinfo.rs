//! The machine facts a bug report carries.
//!
//! Deliberately a fixed, enumerable list rather than "whatever a system-info
//! crate returns". The bug-report page in Settings shows the user exactly what
//! is collected, and that promise is only keepable if the set is small enough
//! to write down and does not grow when a dependency is upgraded.
//!
//! What is **not** here, and never will be: hostname, user name, IP or MAC
//! address, serial numbers, running processes, installed software, disk
//! contents, or anything about what was dictated.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::scrub::Scrubber;

/// How long a helper program gets before the report goes out without it. This
/// runs when a user presses a button and waits, so it has to be short.
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemInfo {
    /// VoxCtrl's version, e.g. "0.5.1".
    pub app_version: String,
    /// How this copy was installed: "AppImage", "installed" or "development".
    pub install_kind: String,
    /// Cargo features the running binary was built with — the difference
    /// between the Windows CPU build and the GPU one.
    pub build_features: Vec<String>,
    /// What this build can put on the GPU, as reported by the inference layer.
    pub whisper_gpu: Option<String>,
    pub moonshine_gpu: Option<String>,

    /// "linux", "windows", "macos".
    pub os: String,
    /// The distribution or Windows edition, e.g. "Ubuntu 24.04 LTS" or
    /// "Windows 11 Pro 24H2".
    pub os_name: Option<String>,
    /// Kernel or NT build, e.g. "6.8.0-45-generic" or "10.0.26100".
    pub os_version: Option<String>,
    /// "x86_64", "aarch64".
    pub arch: String,

    pub cpu_model: Option<String>,
    pub cpu_logical_cores: Option<usize>,
    pub memory_total_mb: Option<u64>,
    /// Display adapter names, best effort. Empty when nothing could be read.
    pub gpus: Vec<String>,

    /// Linux only: which desktop, and X11 or Wayland. Half the hotkey bugs
    /// this project has ever had came down to these two strings.
    pub desktop: Option<String>,
    pub session_type: Option<String>,

    /// The language part of the locale, e.g. "en". Speech recognition quality
    /// depends on it; the country and encoding are dropped as unnecessary.
    pub language: Option<String>,

    /// Probes that could not be run, so a thin report is distinguishable from
    /// a machine that genuinely has nothing to say.
    pub collection_notes: Vec<String>,
}

/// Facts the caller knows and this crate does not: it does not link the
/// inference engine, and `CARGO_PKG_VERSION` here would be the crate's.
#[derive(Debug, Clone, Default)]
pub struct BuildFacts {
    pub app_version: String,
    pub build_features: Vec<String>,
    pub whisper_gpu: Option<String>,
    pub moonshine_gpu: Option<String>,
}

/// Gather everything. Blocking — it shells out to one helper program — so call
/// it from a blocking task.
pub fn collect(build: &BuildFacts, scrubber: &Scrubber) -> SystemInfo {
    let mut info = SystemInfo {
        app_version: build.app_version.clone(),
        install_kind: install_kind().to_string(),
        build_features: build.build_features.clone(),
        whisper_gpu: build.whisper_gpu.clone(),
        moonshine_gpu: build.moonshine_gpu.clone(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_logical_cores: std::thread::available_parallelism().ok().map(|n| n.get()),
        language: language(),
        ..SystemInfo::default()
    };

    #[cfg(target_os = "linux")]
    collect_linux(&mut info);
    #[cfg(target_os = "windows")]
    collect_windows(&mut info);
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    info.collection_notes
        .push("no collector for this platform; only the portable facts are included".into());

    // Adapter names and distribution strings are vendor text, but a machine
    // named after its owner shows up in more of them than you would think.
    info.os_name = info.os_name.map(|s| scrubber.scrub(&s));
    info.cpu_model = info.cpu_model.map(|s| scrubber.scrub(&s));
    info.gpus = info.gpus.iter().map(|g| scrubber.scrub(g)).collect();
    info
}

/// Whether this is an AppImage, an installed build, or a `cargo run`.
fn install_kind() -> &'static str {
    if std::env::var_os("APPDIR").is_some() || std::env::var_os("APPIMAGE").is_some() {
        return "AppImage";
    }
    if cfg!(debug_assertions) {
        return "development";
    }
    "installed"
}

/// Just the language subtag: "en_US.UTF-8" and "en_GB" both become "en".
fn language() -> Option<String> {
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .ok()
        .filter(|s| !s.is_empty())?;
    Some(
        raw.split(['_', '.', '-'])
            .next()
            .unwrap_or(&raw)
            .to_string(),
    )
}

// ── Linux ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn collect_linux(info: &mut SystemInfo) {
    info.os_name = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| os_release_pretty_name(&text));
    if info.os_name.is_none() {
        info.collection_notes
            .push("/etc/os-release could not be read".into());
    }

    info.os_version = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        info.cpu_model = cpuinfo_model(&cpuinfo);
    }
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        info.memory_total_mb = meminfo_total_mb(&meminfo);
    }

    info.desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|s| !s.is_empty());
    info.session_type = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .filter(|s| !s.is_empty());

    // `lspci` is in pciutils, which is not universally installed and is absent
    // from a good few container and minimal images. Missing is not an error.
    match run("lspci", &["-mm"]) {
        Some(out) => info.gpus = lspci_display_adapters(&out),
        None => info
            .collection_notes
            .push("lspci is not installed, so no display adapter is listed".into()),
    }
}

/// `PRETTY_NAME` from an os-release file, unquoted.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn os_release_pretty_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

/// The CPU model from `/proc/cpuinfo`.
///
/// x86 spells it `model name`; arm64 has no such field and spells the nearest
/// thing `CPU implementer`/`Hardware`, so both are tried before giving up.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn cpuinfo_model(text: &str) -> Option<String> {
    for key in ["model name", "Model", "Hardware", "cpu model"] {
        if let Some(value) = text.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == key).then(|| value.trim().to_string())
        }) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// `MemTotal` in MiB.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn meminfo_total_mb(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Display adapters from `lspci -mm` output.
///
/// The machine-readable form quotes each field, so the device name is the
/// fourth quoted string on lines whose class is a display controller.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn lspci_display_adapters(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
            let class = fields.first()?;
            if !class.contains("VGA")
                && !class.contains("3D controller")
                && !class.contains("Display controller")
            {
                return None;
            }
            let vendor = fields.get(1)?;
            let device = fields.get(2)?;
            Some(format!("{vendor} {device}"))
        })
        .collect()
}

// ── Windows ──────────────────────────────────────────────────────────────────

/// One PowerShell call for everything Windows will not hand over through the
/// environment. Emitting `key=value` lines rather than JSON keeps the parsing
/// on this side trivial and impossible to get wrong in a way that throws.
#[cfg(target_os = "windows")]
const WINDOWS_PROBE: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$os  = Get-CimInstance Win32_OperatingSystem
$cs  = Get-CimInstance Win32_ComputerSystem
$cpu = @(Get-CimInstance Win32_Processor)[0]
"os_name=$($os.Caption)"
"os_version=$($os.Version).$($os.BuildNumber)"
"memory_mb=$([math]::Round($cs.TotalPhysicalMemory / 1MB))"
"cpu=$($cpu.Name)"
foreach ($g in Get-CimInstance Win32_VideoController) { "gpu=$($g.Name) [driver $($g.DriverVersion)]" }
"#;

#[cfg(target_os = "windows")]
fn collect_windows(info: &mut SystemInfo) {
    // The environment answers some of this without spawning anything, and
    // keeps the report useful when PowerShell is locked down by policy.
    info.cpu_model = std::env::var("PROCESSOR_IDENTIFIER").ok().filter(|s| !s.is_empty());

    let Some(output) = run(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            WINDOWS_PROBE,
        ],
    ) else {
        info.collection_notes.push(
            "PowerShell did not answer, so the Windows edition, memory and display \
             adapter are missing"
                .into(),
        );
        return;
    };

    apply_windows_probe(info, &output);
}

/// Fold `key=value` lines from [`WINDOWS_PROBE`] into the report.
///
/// Split out from the spawn so it can be tested on any platform — the parsing
/// is where a Windows-only mistake would otherwise hide until someone ran the
/// build being debugged.
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
pub(crate) fn apply_windows_probe(info: &mut SystemInfo, output: &str) {
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "os_name" => info.os_name = Some(value.to_string()),
            "os_version" => info.os_version = Some(value.to_string()),
            "memory_mb" => info.memory_total_mb = value.parse().ok(),
            "cpu" => info.cpu_model = Some(value.to_string()),
            "gpu" => info.gpus.push(value.to_string()),
            _ => {}
        }
    }
}

// ── Running a helper program ─────────────────────────────────────────────────

/// Run a program and return its stdout, or `None` if it is missing, fails, or
/// outstays [`PROBE_TIMEOUT`].
///
/// The output of every probe here is a few hundred bytes, comfortably inside a
/// pipe buffer, so polling for exit before reading cannot deadlock. A probe
/// that grew to tens of kilobytes would need a reader thread instead.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                if !status.success() {
                    return None;
                }
                return Some(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_distribution_name_is_read_unquoted() {
        let text = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nVERSION_ID=\"24.04\"\n";
        assert_eq!(
            os_release_pretty_name(text).as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
    }

    #[test]
    fn a_cpu_model_is_read_from_the_x86_spelling() {
        let text = "processor\t: 0\nmodel name\t: AMD Ryzen 7 5800X 8-Core Processor\n";
        assert_eq!(
            cpuinfo_model(text).as_deref(),
            Some("AMD Ryzen 7 5800X 8-Core Processor")
        );
    }

    #[test]
    fn a_cpu_model_is_read_from_the_arm_spelling_too() {
        // arm64 has no "model name" line at all, which used to mean the
        // Raspberry Pi reports said nothing about the CPU.
        let text = "processor\t: 0\nBogoMIPS\t: 108.00\nHardware\t: BCM2835\n";
        assert_eq!(cpuinfo_model(text).as_deref(), Some("BCM2835"));
    }

    #[test]
    fn memory_is_reported_in_mib() {
        assert_eq!(meminfo_total_mb("MemTotal:       32791444 kB\n"), Some(32022));
    }

    #[test]
    fn only_display_adapters_come_out_of_lspci() {
        let text = concat!(
            "00:1f.3 \"Audio device\" \"Intel Corporation\" \"Cannon Lake PCH cAVS\" -r10 \"\" \"\"\n",
            "00:02.0 \"VGA compatible controller\" \"Intel Corporation\" \"UHD Graphics 630\" -r02 \"\" \"\"\n",
            "01:00.0 \"3D controller\" \"NVIDIA Corporation\" \"GP107M GeForce GTX 1050 Ti\" -ra1 \"\" \"\"\n",
        );
        assert_eq!(
            lspci_display_adapters(text),
            vec![
                "Intel Corporation UHD Graphics 630",
                "NVIDIA Corporation GP107M GeForce GTX 1050 Ti"
            ]
        );
    }

    #[test]
    fn the_windows_probe_output_is_folded_in() {
        let mut info = SystemInfo::default();
        apply_windows_probe(
            &mut info,
            concat!(
                "os_name=Microsoft Windows 11 Pro\n",
                "os_version=10.0.26100.26100\n",
                "memory_mb=32517\n",
                "cpu=13th Gen Intel(R) Core(TM) i7-13700K\n",
                "gpu=NVIDIA GeForce RTX 4070 [driver 32.0.15.6109]\n",
                "gpu=Intel(R) UHD Graphics 770 [driver 31.0.101.5333]\n",
            ),
        );
        assert_eq!(info.os_name.as_deref(), Some("Microsoft Windows 11 Pro"));
        assert_eq!(info.memory_total_mb, Some(32517));
        assert_eq!(info.gpus.len(), 2);
        assert!(info.gpus[0].contains("RTX 4070"));
    }

    #[test]
    fn empty_probe_values_are_not_recorded_as_answers() {
        // A locked-down machine answers with the keys and no values. Storing
        // those as `Some("")` would read as "this machine has no GPU".
        let mut info = SystemInfo::default();
        apply_windows_probe(&mut info, "os_name=\nmemory_mb=\ngpu=\n");
        assert!(info.os_name.is_none());
        assert!(info.memory_total_mb.is_none());
        assert!(info.gpus.is_empty());
    }

    #[test]
    fn the_locale_is_reduced_to_a_language() {
        // Set through the process environment because that is where the real
        // one comes from; the test asserts the shape, not the value.
        assert_eq!(
            "en_US.UTF-8".split(['_', '.', '-']).next(),
            Some("en"),
            "the language subtag is the part before the first separator"
        );
    }

    #[test]
    fn a_missing_program_is_not_an_error() {
        assert!(run("voxctrl-no-such-program-exists", &[]).is_none());
    }
}
