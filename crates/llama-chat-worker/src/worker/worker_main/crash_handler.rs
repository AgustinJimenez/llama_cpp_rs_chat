pub(super) fn install_crash_handler() {
    #[cfg(windows)]
    {
        use std::sync::Once;

        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            unsafe {
                extern "system" fn crash_handler(info: *mut std::ffi::c_void) -> i32 {
                    #[repr(C)]
                    struct ExceptionPointers {
                        record: *const ExceptionRecord,
                        _context: *const std::ffi::c_void,
                    }
                    #[repr(C)]
                    struct ExceptionRecord {
                        code: u32,
                        _flags: u32,
                        _nested: *const std::ffi::c_void,
                        address: *const std::ffi::c_void,
                    }

                    unsafe {
                        let ptrs = info as *const ExceptionPointers;
                        if !ptrs.is_null() && !(*ptrs).record.is_null() {
                            let rec = &*(*ptrs).record;
                            eprintln!(
                                "[CRASH] Exception code: 0x{:08X}, address: {:?}",
                                rec.code, rec.address
                            );
                            if rec.code == 0xC0000005 {
                                eprintln!(
                                    "[CRASH] ACCESS VIOLATION (segfault) — likely CUDA memory corruption"
                                );
                            }
                        } else {
                            eprintln!("[CRASH] Unhandled exception (no details available)");
                        }
                    }
                    eprintln!(
                        "[CRASH] Worker crashing. Check logs/last_prompt_dump.txt, last_inject_dump.txt, last_gen_tokens.txt"
                    );
                    unsafe {
                        let ptrs2 = info as *const ExceptionPointers;
                        if !ptrs2.is_null() && !(*ptrs2).record.is_null() {
                            let code = (*(*ptrs2).record).code;
                            if code == 0xE06D7363 {
                                eprintln!(
                                    "[CRASH] C++ exception — doing controlled exit(42) for fast restart"
                                );
                                std::process::exit(42);
                            }
                        }
                    }
                    0
                }

                extern "system" {
                    fn SetUnhandledExceptionFilter(
                        filter: extern "system" fn(*mut std::ffi::c_void) -> i32,
                    ) -> *mut std::ffi::c_void;
                }
                SetUnhandledExceptionFilter(crash_handler);
                eprintln!("[WORKER] Crash handler installed");
            }
        });
    }

    #[cfg(not(windows))]
    {
        eprintln!("[WORKER] Crash handler not implemented for this platform");
    }
}
