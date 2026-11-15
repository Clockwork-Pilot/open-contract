    use std::fs;
    use std::io;
    use std::path::Path;

    unsafe extern "C" {
        fn __llvm_profile_get_size_for_buffer() -> u64;
        fn __llvm_profile_write_buffer(buffer: *mut i8) -> i32;
        fn __llvm_profile_reset_counters();
        fn __llvm_profile_merge_from_buffer(buffer: *const i8, size: u64) -> i32;
    }

    const MAGIC: &[u8; 8] = b"OCOVCASE";

    pub(super) unsafe fn reset_counters() {
        __llvm_profile_reset_counters();
    }

    pub(super) fn snapshot() -> Result<Vec<u8>, String> {
        let size = unsafe { __llvm_profile_get_size_for_buffer() } as usize;
        let mut bytes = vec![0i8; size];
        let result = unsafe { __llvm_profile_write_buffer(bytes.as_mut_ptr()) };
        if result != 0 {
            return Err(format!("LLVM profile snapshot failed with status {result}"));
        }
        Ok(bytes.into_iter().map(|byte| byte as u8).collect())
    }

    pub(super) fn restore_aggregate(snapshots: &[(String, Vec<u8>)]) -> Result<(), String> {
        unsafe { __llvm_profile_reset_counters() };
        for (id, bytes) in snapshots {
            let result = unsafe {
                __llvm_profile_merge_from_buffer(bytes.as_ptr().cast(), bytes.len() as u64)
            };
            if result != 0 {
                return Err(format!(
                    "LLVM profile merge failed for case {id:?} with status {result}"
                ));
            }
        }
        Ok(())
    }

    pub(super) fn write_snapshots(path: &Path, snapshots: &[(String, Vec<u8>)]) -> io::Result<()> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&(snapshots.len() as u32).to_le_bytes());
        for (id, profile) in snapshots {
            let id = id.as_bytes();
            output.extend_from_slice(&(id.len() as u32).to_le_bytes());
            output.extend_from_slice(&(profile.len() as u64).to_le_bytes());
            output.extend_from_slice(id);
            output.extend_from_slice(profile);
        }
        fs::write(path, output)
    }
