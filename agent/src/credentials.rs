//! Bounded secret input for the GUI's inherited anonymous stdin pipe.
use std::io::{self, Read};
use zeroize::Zeroizing;
pub fn read_secret(input: impl Read) -> io::Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new(Vec::new());
    input.take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "password input too large",
        ));
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid password encoding"))?;
    if text.is_empty() || text.contains(['\r', '\n', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid password input",
        ));
    }
    Ok(Zeroizing::new(text.to_owned()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_pipe_preserves_unicode_and_spaces_without_command_line_arguments() {
        assert_eq!(
            &**read_secret("  密码 😀  \r\n".as_bytes()).unwrap(),
            "  密码 😀  "
        );
        for bytes in [b"".as_slice(), b"first\nsecond", b"x\0y", b"\xff"] {
            assert!(read_secret(bytes).is_err());
        }
    }
    #[test]
    fn secret_pipe_is_bounded() {
        assert!(read_secret(&vec![b'x'; 8193][..]).is_err());
    }
}
