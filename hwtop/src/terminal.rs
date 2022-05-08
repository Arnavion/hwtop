pub(crate) struct Terminal {
	_raw_mode: RawMode,
	_alternate_screen: AlternateScreen,
}

impl Terminal {
	pub(crate) fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, crate::Error> {
		let raw_mode = RawMode::new(stdout)?;
		let alternate_screen = AlternateScreen::new(stdout)?;

		Ok(Terminal {
			_raw_mode: raw_mode,
			_alternate_screen: alternate_screen,
		})
	}
}

struct RawMode {
	stdout_fd: std::os::unix::io::RawFd,
	original_termios: libc::termios,
}

impl RawMode {
	fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, crate::Error> {
		let stdout_fd = std::os::unix::io::AsRawFd::as_raw_fd(stdout);

		let original_termios = unsafe {
			let mut termios = std::mem::zeroed();
			let result = libc::tcgetattr(stdout_fd, &mut termios);
			if result == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			let original_termios = termios;

			libc::cfmakeraw(&mut termios);

			let result = libc::tcsetattr(stdout_fd, 0, &termios);
			if result == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			original_termios
		};

		Ok(RawMode {
			stdout_fd,
			original_termios,
		})
	}
}

impl Drop for RawMode {
	fn drop(&mut self) {
		unsafe {
			let _ = libc::tcsetattr(self.stdout_fd, 0, &self.original_termios);
		}
	}
}

struct AlternateScreen {
	stdout: std::mem::ManuallyDrop<std::fs::File>,
}

impl AlternateScreen {
	fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, crate::Error> {
		std::io::Write::write_all(stdout, b"\x1B[?1049h")?;

		let stdout_fd = std::os::unix::io::AsRawFd::as_raw_fd(stdout);
		let stdout = unsafe { std::os::unix::io::FromRawFd::from_raw_fd(stdout_fd) };
		let stdout = std::mem::ManuallyDrop::new(stdout);

		Ok(AlternateScreen {
			stdout,
		})
	}
}

impl Drop for AlternateScreen {
	fn drop(&mut self) {
		let _ = std::io::Write::write_all(&mut *self.stdout, b"\x1B[?1049l");
	}
}
