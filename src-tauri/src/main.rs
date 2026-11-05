// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--smoke-test") {
        #[cfg(windows)]
        {
            extern "system" {
                fn AttachConsole(dwProcessId: u32) -> i32;
            }
            const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
            unsafe {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }

        let mut out_path = None;
        let mut iter = args.iter().skip(1);
        while let Some(arg) = iter.next() {
            if arg == "--out" {
                if let Some(val) = iter.next() {
                    out_path = Some(std::path::Path::new(val));
                }
            }
        }

        let code = photos_manager_lib::smoke::run_smoke_test(out_path);
        std::process::exit(code);
    }

    photos_manager_lib::run();
}
