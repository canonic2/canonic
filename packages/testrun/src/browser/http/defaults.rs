//! Pinned isolated-request identity without a Node launch or repeated OS probes.
use std::sync::OnceLock;

pub(super) fn isolated_user_agent() -> &'static str {
    static AGENT: OnceLock<String> = OnceLock::new();
    AGENT.get_or_init(|| isolated_agent(product_version().as_deref(), std::env::consts::ARCH))
}

fn isolated_agent(version: Option<&str>, architecture: &str) -> String {
    // The owned driver is pinned to macOS x64 and PW_LANG_NAME=javascript.
    // That spelling selects the pinned embedder fallback unknown/unknown,
    // and its cleared environment never adds CI/1 or version overrides.
    let Some(version) = version else {
        return "Playwright/unknown".into();
    };
    let version = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let architecture = match architecture {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    };
    format!(
        "Playwright/{} ({architecture}; macOS {version}) unknown/unknown",
        crate::browser::DRIVER_VERSION
    )
}

#[cfg(target_os = "macos")]
fn product_version() -> Option<String> {
    let mut version = [0_u8; 128];
    let mut size = version.len();
    // SAFETY: the NUL-terminated name and writable buffer/length have the
    // required sizes; this read-only sysctl owns no external resources.
    if unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            version.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size == 0
        || size > version.len()
    {
        return None;
    }
    let version = std::ffi::CStr::from_bytes_with_nul(&version[..size])
        .ok()?
        .to_str()
        .ok()?;
    if version.is_empty()
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    Some(version.into())
}
#[cfg(not(target_os = "macos"))]
fn product_version() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_user_agent_isolated_matches_pinned_version_truncation_and_failure_fallback() {
        assert_eq!(
            isolated_agent(Some("15.7.2"), "x86_64"),
            "Playwright/1.64.1 (x64; macOS 15.7) unknown/unknown"
        );
        assert_eq!(
            isolated_agent(Some("15"), "aarch64"),
            "Playwright/1.64.1 (arm64; macOS 15) unknown/unknown"
        );
        assert_eq!(isolated_agent(None, "x86_64"), "Playwright/unknown");
    }
}
