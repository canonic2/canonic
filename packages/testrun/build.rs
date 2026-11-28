//! Compile the platform bridges and package the owned runtime assets.
mod build_skills;
mod build_support;
use build_support::{ArtifactCache, dependency_inputs, fingerprint, inputs, local_dependencies};
use std::path::{Path, PathBuf};

fn main() {
    let guides = canonic_mcp_kit::docs::build::embed().expect("Embed Testrun documentation");
    build_skills::embed().expect("Embed Testrun MCP skills");
    // Relative names keep the identity independent of the checkout location.
    runtime_identity(
        guides
            .iter()
            .map(|guide| Path::new("docs").join(&guide.path))
            .collect(),
    );
    android_helper();
    native_regexp();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Unoptimized CLI requests exceed the default Windows main-thread stack.
        println!("cargo:rustc-link-arg-bin=testrun=/STACK:16777216");
        if std::env::var("DEBUG").as_deref() == Ok("false") {
            // Honor the profile's symbol setting when linking native objects.
            println!("cargo:rustc-link-arg=/DEBUG:NONE");
        }
    }
    println!("cargo:rerun-if-changed=native/simulator.m");
    println!("cargo:rerun-if-changed=native/device_media.m");
    println!("cargo:rerun-if-changed=native/audio.m");
    println!("cargo:rerun-if-changed=native/browser_media.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        native_bridge();
        for framework in [
            "Foundation",
            "AppKit",
            "CoreImage",
            "ImageIO",
            "CoreGraphics",
            "IOSurface",
            "VideoToolbox",
            "CoreMedia",
            "CoreVideo",
            "AVFoundation",
            "AudioToolbox",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=native/browser_media_windows.cpp");
        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .file("native/browser_media_windows.cpp")
            .compile("testrun_browser_media");
        println!("cargo:rustc-link-lib=mfuuid");
        println!("cargo:rustc-link-lib=ole32");
    }
}

fn native_bridge() {
    let mut build = cc::Build::new();
    build
        .file("native/simulator.m")
        .file("native/device_media.m")
        .file("native/audio.m")
        .file("native/browser_media.m")
        .flag("-fobjc-arc")
        .flag("-fblocks");
    // Resolve the tools on every run so cc's environment watches remain active.
    let compiler = build.get_compiler().to_command();
    let archiver = build.get_archiver();
    let mut configuration = vec![format!("{compiler:?}"), format!("{archiver:?}")];
    configuration.push(tool_version(compiler.get_program(), "--version"));
    let mut files = inputs(Path::new("native"), &["m", "h"]);
    files.extend(["build.rs".into(), "build_support.rs".into()]);
    let key = fingerprint(&files, &configuration);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let cache = ArtifactCache::new(&out, "libtestrun_simulator.a");
    if cache.current(&key) {
        println!("cargo:rustc-link-lib=static=testrun_simulator");
        println!("cargo:rustc-link-search=native={}", out.display());
    } else {
        cache.invalidate();
        build.compile("testrun_simulator");
        cache.commit(&key);
    }
}

fn native_regexp() {
    let mut build = cc::Build::new();
    build
        .file("native/browser-regexp/bridge.c")
        .file("native/browser-regexp/vendor/libregexp.c")
        .file("native/browser-regexp/vendor/libunicode.c")
        .include("native/browser-regexp/vendor")
        .define("CONFIG_ALL_UNICODE", None)
        .std("c11");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // Strict C11 hides the POSIX clock, pthread and readlink declarations.
        build.define("_POSIX_C_SOURCE", "200809L");
    }
    let tool = build.get_compiler();
    // MSVC's help banner identifies the compiler and exits successfully
    // without a source file. Its --version and /Bv probes exit with errors.
    let version_argument = if tool.is_like_msvc() {
        "/?"
    } else {
        "--version"
    };
    let compiler = tool.to_command();
    let archiver = build.get_archiver();
    let configuration = vec![
        format!("{compiler:?}"),
        format!("{archiver:?}"),
        tool_version(compiler.get_program(), version_argument),
    ];
    let mut files = inputs(Path::new("native/browser-regexp"), &["c", "h"]);
    files.extend(["build.rs".into(), "build_support.rs".into()]);
    let key = fingerprint(&files, &configuration);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let cache = ArtifactCache::new(&out, "libtestrun_browser_regexp.a");
    if cache.current(&key) {
        println!("cargo:rustc-link-lib=static=testrun_browser_regexp");
        println!("cargo:rustc-link-search=native={}", out.display());
    } else {
        cache.invalidate();
        build.compile("testrun_browser_regexp");
        cache.commit(&key);
    }
}

fn tool_version(program: &std::ffi::OsStr, argument: &str) -> String {
    let output = std::process::Command::new(program)
        .arg(argument)
        .output()
        .expect("Read build tool version");
    assert!(output.status.success(), "Read build tool version");
    format!("{:?}{:?}", output.stdout, output.stderr)
}

/// Identify compiled inputs without depending on a Git checkout or build time.
/// `files` starts with the published guides, exactly as the bundle selected them.
fn runtime_identity(mut files: Vec<PathBuf>) {
    files.extend(build_skills::sources(Path::new("skills")));
    for (path, extensions) in [
        ("src", &["rs", "hex", "json", "js"][..]),
        ("probes", &["js", "dart"][..]),
        ("native", &["m", "h", "c", "cpp"][..]),
        ("helper/android", &["java"][..]),
        (
            "runner/ios",
            &["swift", "m", "h", "pbxproj", "xcscheme"][..],
        ),
    ] {
        // Directory watches also detect added/deleted inputs. Stray files may
        // rerun this cheap fingerprint, but never change identity or compile helpers.
        println!("cargo:rerun-if-changed={path}");
        files.extend(inputs(Path::new(path), extensions));
    }
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "build_support.rs",
        "build_skills.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
        files.push(path.into());
    }
    // Shared path dependencies are compiled in without a lockfile checksum.
    for dependency in local_dependencies(Path::new("")) {
        // A missing watched path would rerun this script on every build.
        for watched in ["Cargo.toml", "build.rs", "src"].map(|name| dependency.join(name)) {
            if watched.exists() {
                println!("cargo:rerun-if-changed={}", watched.display());
            }
        }
        files.extend(dependency_inputs(&dependency));
    }
    files.sort();
    let mut configuration = Vec::new();
    for key in [
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
    ] {
        configuration.push(format!("{key}={}", std::env::var(key).unwrap_or_default()));
    }
    let mut features: Vec<_> = std::env::vars()
        .filter(|(name, _)| name.starts_with("CARGO_FEATURE_"))
        .collect();
    features.sort();
    for (name, value) in features {
        configuration.push(format!("{name}={value}"));
    }
    configuration.push(tool_version(
        &std::env::var_os("RUSTC").expect("Rust compiler"),
        "--version",
    ));
    println!(
        "cargo:rustc-env=TESTRUN_BUILD_ID={}",
        fingerprint(&files, &configuration)
    );
}

fn android_sdk() -> PathBuf {
    std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                return PathBuf::from(
                    std::env::var_os("LOCALAPPDATA")
                        .expect("LOCALAPPDATA or ANDROID_HOME required"),
                )
                .join("Android/Sdk");
            }
            let home =
                PathBuf::from(std::env::var_os("HOME").expect("HOME or ANDROID_HOME required"));
            if cfg!(target_os = "macos") {
                home.join("Library/Android/sdk")
            } else {
                home.join("Android/Sdk")
            }
        })
}

fn java_tool(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME").map_or_else(
        || name.into(),
        |home| {
            let executable = PathBuf::from(home).join("bin").join(name);
            if cfg!(windows) {
                executable.with_extension("exe")
            } else {
                executable
            }
        },
    )
}

fn d8_command(tool: &Path) -> std::process::Command {
    use std::process::Command;
    if cfg!(windows) {
        // The Windows build tools ship a batch launcher. Invoke the same
        // pinned D8 jar directly so local and generated paths stay literal.
        let mut command = Command::new(java_tool("java"));
        command.arg("-cp").arg(tool).arg("com.android.tools.r8.D8");
        command
    } else {
        Command::new(tool)
    }
}

fn android_configuration(javac: &Path, sdk_inputs: &[&Path]) -> Vec<String> {
    let mut configuration = vec![
        format!("{javac:?}"),
        tool_version(javac.as_os_str(), "-version"),
        format!("JAVA_HOME={:?}", std::env::var_os("JAVA_HOME")),
    ];
    if cfg!(windows) {
        let java = java_tool("java");
        configuration.push(format!("{java:?}"));
        configuration.push(tool_version(java.as_os_str(), "-version"));
    }
    for path in sdk_inputs {
        println!("cargo:rerun-if-changed={}", path.display());
        // SDK artifacts use Cargo's path/mtime change model; avoid hashing
        // tens of megabytes of SDK binaries on every Rust edit.
        let metadata = std::fs::metadata(path).expect("Inspect Android build tool");
        configuration.push(format!(
            "{path:?}:{}:{:?}",
            metadata.len(),
            metadata.modified().expect("Android build tool timestamp")
        ));
    }
    configuration
}

fn android_helper() {
    use std::process::Command;
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT", "JAVA_HOME"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=helper/android");
    let sdk = android_sdk();
    let jar = sdk.join("platforms/android-36/android.jar");
    let d8_jar = sdk.join("build-tools/36.0.0/lib/d8.jar");
    let d8 = if cfg!(windows) {
        d8_jar.clone()
    } else {
        sdk.join("build-tools/36.0.0/d8")
    };
    assert!(
        jar.is_file() && d8.is_file(),
        "Building Testrun requires Android SDK platform 36 and Build Tools 36.0.0; set ANDROID_HOME"
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let javac = java_tool("javac");
    let mut files = inputs(Path::new("helper/android"), &["java"]);
    let mut sdk_inputs: Vec<&Path> = vec![&jar, &d8_jar];
    if d8 != d8_jar {
        sdk_inputs.push(&d8);
    }
    let configuration = android_configuration(&javac, &sdk_inputs);
    files.extend(["build.rs".into(), "build_support.rs".into()]);
    let key = fingerprint(&files, &configuration);
    let cache = ArtifactCache::new(&out, "classes.dex");
    let focus_cache = ArtifactCache::new(&out, "focus.dex");
    let capture_cache = ArtifactCache::new(&out, "capture.dex");
    if cache.current(&key) && focus_cache.current(&key) && capture_cache.current(&key) {
        return;
    }
    cache.invalidate();
    focus_cache.invalidate();
    capture_cache.invalidate();
    let classes = out.join("media-classes");
    if classes.exists() {
        std::fs::remove_dir_all(&classes).expect("Clean helper classes");
    }
    std::fs::create_dir_all(&classes).expect("Create helper classes");
    assert!(
        Command::new(javac)
            .args([
                "-source",
                "8",
                "-target",
                "8",
                "-Xlint:-options",
                "-bootclasspath"
            ])
            .arg(&jar)
            .arg("-d")
            .arg(&classes)
            .args([
                "helper/android/io/canonic/testrun/media/Shell.java",
                "helper/android/io/canonic/testrun/media/Probe.java",
                "helper/android/io/canonic/testrun/media/Main.java",
                "helper/android/io/canonic/testrun/media/Apps.java",
                "helper/android/io/canonic/testrun/focus/Main.java",
                "helper/android/io/canonic/testrun/focus/Tree.java",
                "helper/android/io/canonic/testrun/capture/Main.java",
                "helper/android/io/canonic/testrun/capture/Renderer.java"
            ])
            .status()
            .expect("Run javac (JDK 17+ required)")
            .success(),
        "Android helper compilation failed"
    );
    let directory = classes.join("io/canonic/testrun/media");
    let mut inputs: Vec<_> = std::fs::read_dir(directory)
        .expect("Read helper classes")
        .map(|e| e.expect("Class entry").path())
        .collect();
    inputs.sort();
    assert!(
        d8_command(&d8)
            .args(["--min-api", "31", "--lib"])
            .arg(&jar)
            .arg("--output")
            .arg(&out)
            .args(inputs)
            .status()
            .expect("Run Android D8")
            .success(),
        "Android DEX compilation failed"
    );
    cache.commit(&key);
    compile_focus_helper(&out, &classes, &jar, &d8);
    focus_cache.commit(&key);
    compile_capture_helper(&out, &classes, &jar, &d8);
    capture_cache.commit(&key);
}

fn compile_capture_helper(out: &Path, classes: &Path, jar: &Path, d8: &Path) {
    let output = out.join("capture-dex");
    std::fs::create_dir_all(&output).expect("Create capture DEX output");
    let mut files: Vec<_> = std::fs::read_dir(classes.join("io/canonic/testrun/capture"))
        .expect("Read capture helper classes")
        .map(|entry| entry.expect("Capture class entry").path())
        .collect();
    files.sort();
    assert!(
        d8_command(d8)
            .args(["--min-api", "31", "--lib"])
            .arg(jar)
            .arg("--output")
            .arg(&output)
            .args(files)
            .status()
            .expect("Run capture D8")
            .success(),
        "Capture DEX compilation failed"
    );
    std::fs::rename(output.join("classes.dex"), out.join("capture.dex"))
        .expect("Publish capture DEX");
}

fn compile_focus_helper(out: &Path, classes: &Path, jar: &Path, d8: &Path) {
    let focus_out = out.join("focus-dex");
    std::fs::create_dir_all(&focus_out).expect("Create focus DEX output");
    let mut focus_inputs: Vec<_> = std::fs::read_dir(classes.join("io/canonic/testrun/focus"))
        .expect("Read focus helper classes")
        .map(|entry| entry.expect("Focus class entry").path())
        .collect();
    focus_inputs.sort();
    assert!(
        d8_command(d8)
            .args(["--min-api", "24", "--lib"])
            .arg(jar)
            .arg("--output")
            .arg(&focus_out)
            .args(focus_inputs)
            .status()
            .expect("Run focus D8")
            .success(),
        "Focus DEX compilation failed"
    );
    std::fs::rename(focus_out.join("classes.dex"), out.join("focus.dex"))
        .expect("Publish focus DEX");
}
