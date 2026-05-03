//! PawnIO kernel driver client.
//!
//! PawnIO (`PawnIO.sys`, https://pawnio.eu) is a signed Windows kernel
//! driver that executes sandboxed Pawn bytecode modules to expose
//! privileged hardware access (MSR reads, SMN reads, port I/O) to user
//! mode. Modules are cryptographically signed and validated by the driver.
//!
//! This module wraps the user-mode IOCTL protocol. Each [`PawnIo`]
//! instance opens the device, loads one bytecode module, and exposes
//! convenience methods for reading MSRs and SMN registers. The handle
//! and module are released when the struct is dropped.
//!
//! The embedded `IntelMSR.bin` and `AMDFamily17.bin` modules are taken
//! verbatim from PawnIO.Modules release 0.2.5, licensed under
//! LGPL-2.1-or-later. See `COPYING.LGPL-2.1` in this directory.

use crate::collect::win::OwnedHandle;
use std::ffi::c_void;
use thiserror::Error;
use windows::Win32::Foundation::HANDLE;

// ---------------------------------------------------------------------------
// PawnIO IOCTL protocol constants (verified from pawnio_um.h)
// ---------------------------------------------------------------------------

/// IOCTL: load a signed Pawn bytecode module into the driver VM.
const IOCTL_PIO_LOAD_BINARY: u32 = 0xA1B2_2084;

/// IOCTL: execute a named function in the loaded module.
const IOCTL_PIO_EXECUTE_FN: u32 = 0xA1B2_2104;

/// Function name field length in `IOCTL_PIO_EXECUTE_FN` input buffers.
const FN_NAME_LEN: usize = 32;

/// Cell size in bytes (PawnIO uses 64-bit cells).
const CELL_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Embedded signed modules (LGPL-2.1-or-later — see COPYING.LGPL-2.1)
// ---------------------------------------------------------------------------

const INTEL_MSR_BIN: &[u8] = include_bytes!("IntelMSR.bin");
const AMD_FAMILY17_BIN: &[u8] = include_bytes!("AMDFamily17.bin");

/// PawnIO bytecode module to load.
#[derive(Clone, Copy)]
pub enum Module {
    /// Intel MSR access — `ioctl_read_msr` for any allowlisted MSR.
    IntelMsr,
    /// AMD Family 17h+ access — `ioctl_read_msr` and `ioctl_read_smn`.
    AmdFamily17,
}

impl Module {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::IntelMsr => INTEL_MSR_BIN,
            Self::AmdFamily17 => AMD_FAMILY17_BIN,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the PawnIO client.
#[derive(Debug, Error)]
pub enum Error {
    /// PawnIO device could not be opened (driver not installed).
    #[error("PawnIO device not available")]
    DeviceUnavailable,
    /// `IOCTL_PIO_LOAD_BINARY` failed (bad signature or unsupported CPU).
    #[error("PawnIO module load failed (error {0:#x})")]
    LoadFailed(i32),
    /// `IOCTL_PIO_EXECUTE_FN` returned a failure status from the driver.
    #[error("PawnIO function execute failed (error {0:#x})")]
    ExecuteFailed(i32),
    /// The driver returned fewer output bytes than the caller requested.
    #[error("PawnIO short read: expected {expected} bytes, got {actual}")]
    ShortRead { expected: usize, actual: usize },
}

// ---------------------------------------------------------------------------
// PawnIo client
// ---------------------------------------------------------------------------

/// An open PawnIO device handle with a loaded bytecode module.
///
/// Each instance owns a single `\Device\PawnIO` handle and exactly one loaded
/// module. The driver destroys its VM when the handle is closed (on drop).
pub struct PawnIo {
    handle: OwnedHandle,
}

impl PawnIo {
    /// Open the PawnIO device and load the requested bytecode module.
    pub fn load(module: Module) -> Result<Self, Error> {
        let handle = open_device()?;
        load_binary(handle.get(), module.bytes())?;
        Ok(Self { handle })
    }

    /// Execute a named IOCTL function in the loaded module.
    ///
    /// The `input` and `output` slices are 64-bit cells. The driver writes
    /// `output.len()` cells; partial responses are reported as
    /// [`Error::ShortRead`].
    fn execute(&self, name: &str, input: &[u64], output: &mut [u64]) -> Result<(), Error> {
        let mut input_buf = vec![0u8; FN_NAME_LEN + input.len() * CELL_SIZE];
        let name_bytes = name.as_bytes();
        let copy_len = name_bytes.len().min(FN_NAME_LEN - 1);
        input_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        for (i, &cell) in input.iter().enumerate() {
            let off = FN_NAME_LEN + i * CELL_SIZE;
            input_buf[off..off + CELL_SIZE].copy_from_slice(&cell.to_le_bytes());
        }

        // SAFETY: output is a mutable slice of u64; reinterpreting as bytes is
        // sound for any bit pattern (u64 is plain old data) and the byte length
        // is exactly output.len() * 8.
        let output_bytes: &mut [u8] = unsafe {
            std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u8, output.len() * CELL_SIZE)
        };

        let bytes_returned = device_io_control(
            self.handle.get(),
            IOCTL_PIO_EXECUTE_FN,
            &input_buf,
            output_bytes,
        )?;

        if bytes_returned < output_bytes.len() {
            return Err(Error::ShortRead {
                expected: output_bytes.len(),
                actual: bytes_returned,
            });
        }
        Ok(())
    }

    /// Read a Model-Specific Register on the calling thread's current core.
    ///
    /// To target a specific core, install an [`AffinityGuard`] on the calling
    /// thread before invoking this method.
    pub fn read_msr(&self, msr: u32) -> Result<u64, Error> {
        let mut output = [0u64; 1];
        self.execute("ioctl_read_msr", &[msr as u64], &mut output)?;
        Ok(output[0])
    }

    /// Read a 32-bit System Management Network register (AMD only).
    pub fn read_smn(&self, address: u32) -> Result<u32, Error> {
        let mut output = [0u64; 1];
        self.execute("ioctl_read_smn", &[address as u64], &mut output)?;
        Ok(output[0] as u32)
    }
}

// ---------------------------------------------------------------------------
// Device open / IOCTL helpers
// ---------------------------------------------------------------------------

fn open_device() -> Result<OwnedHandle, Error> {
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };

    let path: Vec<u16> = r"\\?\GLOBALROOT\Device\PawnIO"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: CreateFileW receives a valid null-terminated UTF-16 path.
    // The returned HANDLE is wrapped in OwnedHandle which closes it on drop.
    let raw = unsafe {
        CreateFileW(
            windows::core::PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|_| Error::DeviceUnavailable)?;

    OwnedHandle::new(raw).ok_or(Error::DeviceUnavailable)
}

fn load_binary(handle: HANDLE, bin: &[u8]) -> Result<(), Error> {
    let mut empty: [u8; 0] = [];
    let _ =
        device_io_control(handle, IOCTL_PIO_LOAD_BINARY, bin, &mut empty).map_err(|e| match e {
            Error::ExecuteFailed(code) => Error::LoadFailed(code),
            other => other,
        })?;
    Ok(())
}

/// Wrapper around `DeviceIoControl` that returns bytes written or an error.
fn device_io_control(
    handle: HANDLE,
    code: u32,
    input: &[u8],
    output: &mut [u8],
) -> Result<usize, Error> {
    use windows::Win32::System::IO::DeviceIoControl;

    let in_ptr: Option<*const c_void> = if input.is_empty() {
        None
    } else {
        Some(input.as_ptr() as *const c_void)
    };
    let out_ptr: Option<*mut c_void> = if output.is_empty() {
        None
    } else {
        Some(output.as_mut_ptr() as *mut c_void)
    };
    let mut bytes_returned: u32 = 0;

    // SAFETY: handle is a valid open device handle. input and output point to
    // properly sized buffers (or are None when their length is zero). The
    // driver uses METHOD_BUFFERED so the kernel manages the copy in/out.
    let result = unsafe {
        DeviceIoControl(
            handle,
            code,
            in_ptr,
            input.len() as u32,
            out_ptr,
            output.len() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    result
        .map(|_| bytes_returned as usize)
        .map_err(|e| Error::ExecuteFailed(e.code().0))
}

// ---------------------------------------------------------------------------
// Thread affinity guard for per-core MSR reads
// ---------------------------------------------------------------------------

/// RAII guard that pins the calling thread to a specific processor and
/// restores the previous affinity on drop.
///
/// Uses `SetThreadGroupAffinity` so it works correctly on systems with more
/// than 64 logical processors (multiple processor groups).
pub struct AffinityGuard {
    previous: windows::Win32::System::SystemInformation::GROUP_AFFINITY,
    restored: bool,
}

impl AffinityGuard {
    /// Pin the current thread to the given processor group and CPU mask.
    pub fn pin(group: u16, mask: usize) -> Result<Self, Error> {
        use windows::Win32::System::SystemInformation::GROUP_AFFINITY;
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadGroupAffinity};

        let new = GROUP_AFFINITY {
            Mask: mask,
            Group: group,
            Reserved: [0; 3],
        };
        let mut previous = GROUP_AFFINITY::default();

        // SAFETY: GetCurrentThread is a pseudo-handle (no resource), and the
        // GROUP_AFFINITY pointers are valid stack-allocated structs.
        let ok = unsafe { SetThreadGroupAffinity(GetCurrentThread(), &new, Some(&mut previous)) };
        if !ok.as_bool() {
            // SAFETY: GetLastError reads thread-local state and is always safe
            // to call; the unsafe annotation reflects FFI binding convention.
            let last = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(Error::ExecuteFailed(last.0 as i32));
        }

        Ok(Self {
            previous,
            restored: false,
        })
    }
}

impl Drop for AffinityGuard {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadGroupAffinity};
        // SAFETY: previous holds a valid GROUP_AFFINITY captured at pin().
        // GetCurrentThread is a pseudo-handle. Failure to restore affinity is
        // ignored (the worst case is a thread pinned to one core).
        let _ = unsafe { SetThreadGroupAffinity(GetCurrentThread(), &self.previous, None) };
        self.restored = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_codes_match_pawnio_protocol() {
        // Verified against namazso/PawnIO PawnIO/include/pawnio_um.h
        // CTL_CODE(0xA1B2, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS) = 0xA1B22084
        // CTL_CODE(0xA1B2, 0x841, METHOD_BUFFERED, FILE_ANY_ACCESS) = 0xA1B22104
        assert_eq!(IOCTL_PIO_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_PIO_EXECUTE_FN, 0xA1B2_2104);
    }

    #[test]
    fn embedded_modules_are_non_empty() {
        assert!(!INTEL_MSR_BIN.is_empty());
        assert!(!AMD_FAMILY17_BIN.is_empty());
    }
}
