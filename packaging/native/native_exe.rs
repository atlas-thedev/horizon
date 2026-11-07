// Native Client: the name and icon of the Windows executable (`tools/native/native_client.py` copies this file next
// to the client's build.rs and includes it there).
//
// Windows shows FileDescription in Task Manager and in the firewall prompt ("Windows Defender Firewall has blocked
// some features of this app"), so it says Native Client, not the engine's name. A build on Windows sets these strings
// through winresource; the Linux cross build (tools/native/vps_build.sh), where winresource does not run, writes the
// same resource script and compiles it with mingw's windres.

/// The version strings of the executable.
#[allow(dead_code)]
const NATIVE_EXE_STRINGS: [(&str, &str); 6] = [
    ("CompanyName", "Native"),
    ("FileDescription", "Native Client"),
    ("ProductName", "Native Client"),
    ("InternalName", "native-client"),
    ("OriginalFilename", "native-client.exe"),
    ("LegalCopyright", "Native Client, built on the Horizon project (MIT)"),
];

/// The icon and version resource for a Windows target built on another system: windres turns the script into an
/// object the linker takes like any other. Without windres the build goes on, without them.
#[allow(dead_code)]
fn native_exe_resources(ico: &str, out_dir: &std::path::Path) {
    if cfg!(windows) || std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = std::fs::canonicalize(ico).unwrap_or_else(|_| std::path::PathBuf::from(ico));
    let quote = |s: &str| s.replace('\\', "/").replace('"', "\"\"");
    let mut strings = String::new();
    for (k, v) in NATIVE_EXE_STRINGS {
        strings.push_str(&format!("      VALUE \"{k}\", \"{}\"\n", quote(v)));
    }
    let script = format!(
        "1 ICON \"{icon}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION 1,0,0,0\n\
         PRODUCTVERSION 1,0,0,0\n\
         FILEOS 0x40004\n\
         FILETYPE 0x1\n\
         BEGIN\n\
         \x20 BLOCK \"StringFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   BLOCK \"040904B0\"\n\
         \x20   BEGIN\n\
         {strings}\
         \x20     VALUE \"FileVersion\", \"1.0.0.0\"\n\
         \x20     VALUE \"ProductVersion\", \"1.0.0.0\"\n\
         \x20   END\n\
         \x20 END\n\
         \x20 BLOCK \"VarFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   VALUE \"Translation\", 0x409, 1200\n\
         \x20 END\n\
         END\n",
        icon = quote(&ico.display().to_string()),
    );
    let rc = out_dir.join("native-exe.rc");
    let object = out_dir.join("native-exe-res.o");
    if std::fs::write(&rc, script).is_err() {
        return;
    }
    let windres = std::env::var("NATIVE_WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let built = std::process::Command::new(&windres)
        .arg(&rc)
        .args(["-O", "coff", "-o"])
        .arg(&object)
        .status()
        .is_ok_and(|s| s.success());
    if built {
        println!("cargo:rustc-link-arg-bin=horizon-client={}", object.display());
    } else {
        println!("cargo:warning=Native Client: {windres} failed, the executable has no icon or name");
    }
}
