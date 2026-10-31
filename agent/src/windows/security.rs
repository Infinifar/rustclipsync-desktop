use super::{wide, Handle};
use std::{io, mem, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, *},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
use zeroize::Zeroizing;

/// Owns a self-relative descriptor with a protected DACL for the current user.
pub struct Security {
    descriptor: PSECURITY_DESCRIPTOR,
}
impl Security {
    pub fn current_user() -> io::Result<Self> {
        unsafe {
            let mut raw = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Handle::new(raw)?;
            let mut needed = 0;
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            // usize storage provides alignment for TOKEN_USER.
            let mut buffer = vec![0usize; (needed as usize).div_ceil(mem::size_of::<usize>())];
            if GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0;
            while *sid.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
            LocalFree(sid.cast());
            let sddl = wide(&format!("D:P(A;OICI;FA;;;{name})"));
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { descriptor })
        }
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }
    pub fn protect_path(&self, path: &std::path::Path) -> io::Result<()> {
        unsafe {
            let path = super::path_wide(path)?;
            if SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                self.descriptor,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}

pub fn protect(bytes: &[u8]) -> io::Result<Vec<u8>> {
    crypt(bytes, false).map(|v| v.to_vec())
}
pub fn unprotect(bytes: &[u8]) -> io::Result<Zeroizing<Vec<u8>>> {
    crypt(bytes, true)
}
fn crypt(bytes: &[u8], decrypt: bool) -> io::Result<Zeroizing<Vec<u8>>> {
    unsafe {
        use windows_sys::Win32::Security::Cryptography::*;
        if bytes.len() > 64 * 1024 {
            return Err(io::Error::other("credential blob too large"));
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
        };
        let mut output = mem::zeroed();
        let success = if decrypt {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if success == 0 {
            return Err(io::Error::other("DPAPI operation failed"));
        }
        let result = Zeroizing::new(if output.cbData == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        });
        for i in 0..output.cbData as usize {
            ptr::write_volatile(output.pbData.add(i), 0);
        }
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

fn token_sid(logon: bool) -> io::Result<String> {
    unsafe {
        let mut raw = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle::new(raw)?;
        let class = if logon { TokenLogonSid } else { TokenUser };
        let mut needed = 0;
        GetTokenInformation(token.0, class, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(mem::size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            class,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let sid = if logon {
            let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
            if groups.GroupCount != 1 {
                return Err(io::Error::other("missing logon identity"));
            }
            groups.Groups[0].Sid
        } else {
            (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid
        };
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(result)
    }
}
pub fn user_sid() -> io::Result<String> {
    token_sid(false)
}
impl Security {
    pub fn current_logon() -> io::Result<Self> {
        unsafe {
            let sid = token_sid(true)?;
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(&format!("D:P(A;;GRGW;;;{sid})")).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { descriptor })
        }
    }
}
