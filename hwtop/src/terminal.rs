pub(crate) fn make(stdout: impl std::io::Write + std::os::unix::io::AsRawFd) -> Result<impl std::io::Write, crate::Error> {
	let raw_mode = RawMode::new(stdout)?;
	let alternate_screen = AlternateScreen::new(raw_mode)?;
	Ok(alternate_screen)
}

struct RawMode<W> where W: std::io::Write + std::os::unix::io::AsRawFd {
	inner: W,
	original_termios: libc::termios,
}

impl<W> RawMode<W> where W: std::io::Write + std::os::unix::io::AsRawFd {
	fn new(inner: W) -> Result<Self, crate::Error> {
		let original_termios = unsafe {
			let inner_fd = std::os::unix::io::AsRawFd::as_raw_fd(&inner);

			let mut termios = std::mem::zeroed();
			let result = libc::tcgetattr(inner_fd, &mut termios);
			if result == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			let original_termios = termios;

			libc::cfmakeraw(&mut termios);

			let result = libc::tcsetattr(inner_fd, 0, &termios);
			if result == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			original_termios
		};

		Ok(RawMode {
			inner,
			original_termios,
		})
	}
}

impl<W> std::io::Write for RawMode<W> where W: std::io::Write + std::os::unix::io::AsRawFd {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.inner.write(buf)
	}

	fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
		self.inner.write_vectored(bufs)
	}

	fn flush(&mut self) -> std::io::Result<()> {
		self.inner.flush()
	}

	fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
		self.inner.write_all(buf)
	}
}

impl<W> Drop for RawMode<W> where W: std::io::Write + std::os::unix::io::AsRawFd {
	fn drop(&mut self) {
		unsafe {
			let inner_fd = std::os::unix::io::AsRawFd::as_raw_fd(&self.inner);
			let _ = libc::tcsetattr(inner_fd, 0, &self.original_termios);
		}
	}
}

struct AlternateScreen<W> where W: std::io::Write {
	inner: W,
}

impl<W> AlternateScreen<W> where W: std::io::Write {
	fn new(mut inner: W) -> Result<Self, crate::Error> {
		std::io::Write::write_all(&mut inner, b"\x1B[?1049h")?;
		Ok(AlternateScreen {
			inner,
		})
	}
}

impl<W> std::io::Write for AlternateScreen<W> where W: std::io::Write {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.inner.write(buf)
	}

	fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
		self.inner.write_vectored(bufs)
	}

	fn flush(&mut self) -> std::io::Result<()> {
		self.inner.flush()
	}

	fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
		self.inner.write_all(buf)
	}
}

impl<W> Drop for AlternateScreen<W> where W: std::io::Write {
	fn drop(&mut self) {
		let _ = std::io::Write::write_all(&mut self.inner, b"\x1B[?1049l");
	}
}
