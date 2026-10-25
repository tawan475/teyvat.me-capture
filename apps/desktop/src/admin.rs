// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

// The code from https://github.com/IceDynamix/reliquary-archiver
//
// MIT License
//
// Copyright (c) 2024 IceDynamix
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

/// Report whether the process has the privileges raw packet capture needs.
///
/// On Windows, elevation is owned entirely by the application manifest that
/// `build.rs` embeds into the binary (`requestedExecutionLevel
/// level="requireAdministrator"`). Windows evaluates that manifest before
/// `main` runs: it shows the UAC prompt itself, and refuses to start the
/// process at all if the prompt is declined. There is consequently nothing for
/// this function to escalate — it used to re-launch itself through
/// `ShellExecuteExW("runas")`, which was dead code in every shipped build and
/// carried its own bugs (arguments were re-joined with spaces without
/// `CommandLineToArgvW` quoting, and the process exited with status 0 even
/// when the re-launch failed).
///
/// A consequence of the manifest owning elevation is that `--no-admin` cannot
/// do anything on Windows: the OS has already decided by the time the flag is
/// parsed. Keep the flag's help text honest about that (see `main.rs`).
#[cfg(windows)]
pub fn ensure_admin() {
    if unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().into() } {
        tracing::info!("Running with admin privileges");
        return;
    }

    // Only reachable in a build whose manifest did not apply, e.g. one where
    // the resource compiler was unavailable. Packet capture will fail, so say
    // so plainly rather than silently relaunching or exiting.
    tracing::warn!(
        "Not running with admin privileges even though the embedded manifest requires them; \
         packet capture will fail. Restart Teyvat.me with \"Run as administrator\"."
    );
}

/// Set by the UI's "Restart as administrator": `main`'s relaunch then asks
/// Windows to elevate the new copy instead of starting it plainly. Done
/// there, after the window closed and the single-instance locks are free; a
/// copy started from inside the app would find them held and exit.
#[cfg(windows)]
pub static ELEVATE_ON_RESTART: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Start the running exe again through the `runas` verb (UAC), with `args`
/// minus `--no-admin`. Returns whether Windows started it.
#[cfg(windows)]
pub fn relaunch_elevated(args: &[std::ffi::OsString]) -> bool {
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let parameters: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .filter(|arg| arg != "--no-admin")
        .map(|arg| quote_windows_arg(&arg))
        .collect();
    let file = HSTRING::from(exe.as_os_str());
    let parameters = HSTRING::from(parameters.join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: every pointer in `info` outlives the call.
    match unsafe { ShellExecuteExW(&mut info) } {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("could not relaunch as administrator: {e}");
            false
        }
    }
}

/// One argument quoted the way `CommandLineToArgvW` reads it back: quotes
/// around anything with spaces, tabs or quotes, backslashes doubled before a
/// quote.
#[cfg(any(windows, test))]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod quote_tests {
    use super::quote_windows_arg;

    #[test]
    fn arguments_are_quoted_as_command_line_to_argv_reads_them() {
        assert_eq!(quote_windows_arg("-b"), "-b");
        assert_eq!(quote_windows_arg(""), "\"\"");
        assert_eq!(
            quote_windows_arg(r"D:\My Captures\a.pcap"),
            r#""D:\My Captures\a.pcap""#
        );
        assert_eq!(quote_windows_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(
            quote_windows_arg(r"C:\dir with space\"),
            r#""C:\dir with space\\""#
        );
    }
}

#[cfg(unix)]
pub fn ensure_admin() {
    // Running as root is always sufficient
    let is_root = unsafe { libc::geteuid() } == 0;
    if is_root {
        return;
    }

    // On Linux, CAP_NET_RAW is sufficient
    #[cfg(target_os = "linux")]
    if caps::has_cap(None, caps::CapSet::Effective, caps::Capability::CAP_NET_RAW)
        .is_ok_and(|has_net_raw| has_net_raw)
    {
        return;
    }

    // On macOS, /dev/bpf access is sufficient
    #[cfg(target_os = "macos")]
    {
        use std::io::ErrorKind;

        // Each capturing process takes exclusive ownership of one bpf device,
        // so /dev/bpf0 alone says nothing: if Wireshark (or any other libpcap
        // consumer) holds it, opening it fails with EBUSY even though our
        // permissions are fine. Probe the first few devices and treat "busy"
        // as proof that permissions are correct, so only a genuine permission
        // failure on every device reaches the dialog below.
        for n in 0..=9 {
            match std::fs::File::open(format!("/dev/bpf{n}")) {
                Ok(_) => return,
                Err(e) if e.kind() == ErrorKind::ResourceBusy => return,
                Err(_) => continue,
            }
        }
    }

    show_packet_capture_permissions_missing_dialog();
}

#[cfg(unix)]
fn show_packet_capture_permissions_missing_dialog() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([500.0, 200.0])
            .with_resizable(true),
        ..Default::default()
    };

    // Try to get the current executable path
    let exe_path = std::env::current_exe()
        .ok()
        .map(|mut path| path.as_mut_os_string().to_string_lossy().to_string())
        .unwrap_or("./teyvatme".to_owned());

    let _ = eframe::run_simple_native(
        "Teyvat.me requires packet capture permissions",
        options,
        move |ctx, _frame| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label("How to grant packet capture permissions:");
                    ui.add_space(5.0);

                    #[cfg(target_os = "linux")]
                    {
                        ui.label("1. Grant CAP_NET_RAW to Teyvat.me (after every update):");
                        ui.label(format!(
                            "sudo setcap cap_net_raw=ep '{}' && '{}'",
                            exe_path, exe_path
                        ));
                    }

                    #[cfg(target_os = "macos")]
                    {
                        ui.label("1. Grant read permissions on /dev/bpf* (after every reboot):");
                        ui.label("sudo chmod 644 /dev/bpf*");
                    }

                    ui.add_space(5.0);
                    ui.label("2. Run Teyvat.me as root (every time):");
                    ui.label(format!("sudo '{}'", exe_path));
                    ui.add_space(10.0);
                    ui.label("Rerun Teyvat.me with --no-admin if you wish to proceed without packet capture")
                });

                // Push button to the bottom
                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::Center).with_cross_justify(true),
                    |ui| {
                        ui.add_space(10.0); // Small margin from bottom edge
                        if ui.button("OK").clicked() {
                            crate::crash::mark_clean_exit(PERMISSIONS_MISSING);
                            std::process::exit(1);
                        }
                    },
                );
            });
        },
    );

    crate::crash::mark_clean_exit(PERMISSIONS_MISSING);
    std::process::exit(1);
}

/// The clean-exit reason when the permissions dialog ends the process; without
/// a marker the next start would call this exit abrupt.
#[cfg(unix)]
const PERMISSIONS_MISSING: &str = "packet capture permissions are missing";
