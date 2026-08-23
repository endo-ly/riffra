use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    ERROR_PIPE_CONNECTED, GetLastError, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};

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
        let sddl: Vec<u16> = "D:P(A;;GA;;;CO)"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
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
}
