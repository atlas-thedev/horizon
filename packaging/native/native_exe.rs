// Native Client: the name of the Windows executable (`tools/native/native_client.py` copies this file next to the
// client's build.rs, includes it there and adds these strings to the resource upstream compiles with the icon).
//
// Windows shows FileDescription in Task Manager and in the firewall prompt ("Windows Defender Firewall has blocked
// some features of this app"), so it says Native Client, not the engine's name.

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
