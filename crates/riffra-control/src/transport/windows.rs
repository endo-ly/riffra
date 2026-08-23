use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::null_mut;

#[cfg(test)]
use windows_sys::Win32::Foundation::GENERIC_ALL;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION,
};
#[cfg(test)]
use windows_sys::Win32::Security::{ACCESS_ALLOWED_ACE, GetAce, GetSecurityDescriptorDacl};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Blocking listener for one user-scoped Windows Named Pipe.
pub struct NamedPipeListener {
    pipe_name: Vec<u16>,
    pending_instance: Option<File>,
}

impl NamedPipeListener {
    /// Creates the first pipe instance, proving that the endpoint can bind.
    pub fn bind(pipe_name: &str) -> io::Result<Self> {
        let mut listener = Self {
            pipe_name: wide(pipe_name),
            pending_instance: None,
        };
        listener.pending_instance = Some(listener.create_instance()?);
        Ok(listener)
    }

    /// Blocks until a client connects and returns its byte stream.
    pub fn accept(&mut self) -> io::Result<File> {
        let stream = match self.pending_instance.take() {
            Some(stream) => stream,
            None => self.create_instance()?,
        };
        let connected = unsafe { ConnectNamedPipe(stream.as_raw_handle() as _, null_mut()) };
        if connected != 0 {
            return Ok(stream);
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_PIPE_CONNECTED {
            Ok(stream)
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    }

    fn create_instance(&self) -> io::Result<File> {
        let security = PipeSecurity::new()?;
        let handle = unsafe {
            CreateNamedPipeW(
                self.pipe_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                255,
                super::MAX_FRAME_SIZE as u32,
                super::MAX_FRAME_SIZE as u32,
                0,
                &security.attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateNamedPipeW returned a valid owned handle. File closes it.
        Ok(unsafe { File::from_raw_handle(handle as _) })
    }
}

struct PipeSecurity {
    attributes: SECURITY_ATTRIBUTES,
    descriptor: PSECURITY_DESCRIPTOR,
}

impl PipeSecurity {
    fn new() -> io::Result<Self> {
        let mut descriptor = null_mut();
        let sddl = current_user_sddl()?;
        let encoded_sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded_sddl.as_ptr(),
                SDDL_REVISION,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
            descriptor,
        })
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(self.descriptor as HLOCAL);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = TokenHandle(token);

    let mut required = 0;
    let first_call =
        unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required) };
    let first_error = unsafe { GetLastError() };
    if first_call != 0 || first_error != ERROR_INSUFFICIENT_BUFFER || required == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = vec![0_u8; required as usize];
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    let token_user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
    sid_to_string(token_user.User.Sid)
}

fn sid_to_string(sid: *mut core::ffi::c_void) -> io::Result<String> {
    let mut sid_string = std::ptr::null_mut();
    let converted = unsafe { ConvertSidToStringSidW(sid, &mut sid_string) };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    let sid = unsafe {
        let mut length = 0;
        while *sid_string.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(sid_string, length))
    };
    unsafe {
        let _ = LocalFree(sid_string as HLOCAL);
    }
    Ok(sid)
}

fn current_user_sddl() -> io::Result<String> {
    Ok(format!("D:P(A;;GA;;;{})", current_user_sid()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{connect, read_frame, write_frame};
    use serde_json::Value;
    use std::{thread, time::Duration};

    #[test]
    fn named_pipe_round_trips_and_accepts_reconnect() {
        let name = crate::pipe_name(&format!(
            "test-{}-{}",
            std::process::id(),
            crate::new_instance_id()
        ));
        let mut listener = NamedPipeListener::bind(&name).unwrap();
        let server = thread::spawn(move || {
            for expected in [1, 2] {
                let mut stream = listener.accept().unwrap();
                let request: Value = read_frame(&mut stream).unwrap();
                assert_eq!(request["request"], expected);
                write_frame(&mut stream, &serde_json::json!({"request": expected})).unwrap();
            }
        });

        for expected in [1, 2] {
            let mut client = (0..200)
                .find_map(|_| match connect(&name) {
                    Ok(stream) => Some(stream),
                    Err(_) => {
                        thread::sleep(Duration::from_millis(5));
                        None
                    }
                })
                .expect("named pipe client should connect");
            write_frame(&mut client, &serde_json::json!({"request": expected})).unwrap();
            let response: Value = read_frame(&mut client).unwrap();
            assert_eq!(response["request"], expected);
        }

        server.join().unwrap();
    }

    #[test]
    fn pipe_security_allows_the_current_user_sid() {
        let security = PipeSecurity::new().unwrap();
        let (descriptor_sid, access_mask) = security.allowed_sid_and_mask().unwrap();

        assert_eq!(descriptor_sid, current_user_sid().unwrap());
        assert_eq!(access_mask, GENERIC_ALL);
    }

    impl PipeSecurity {
        fn allowed_sid_and_mask(&self) -> io::Result<(String, u32)> {
            let mut dacl_present = 0;
            let mut dacl = null_mut();
            let mut dacl_defaulted = 0;
            let descriptor_read = unsafe {
                GetSecurityDescriptorDacl(
                    self.descriptor,
                    &mut dacl_present,
                    &mut dacl,
                    &mut dacl_defaulted,
                )
            };
            if descriptor_read == 0 {
                return Err(io::Error::last_os_error());
            }
            if dacl_present == 0 || dacl.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pipe security descriptor has no DACL",
                ));
            }

            let mut ace = null_mut();
            let ace_read = unsafe { GetAce(dacl, 0, &mut ace) };
            if ace_read == 0 || ace.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pipe security descriptor has no access rule",
                ));
            }
            let allowed_ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            let sid = std::ptr::addr_of!(allowed_ace.SidStart).cast_mut().cast();
            Ok((sid_to_string(sid)?, allowed_ace.Mask))
        }
    }
}
