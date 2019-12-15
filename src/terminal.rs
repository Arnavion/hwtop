pub(crate) struct Terminal {
	_raw_mode: RawMode,
	_alternate_screen: AlternateScreen,
	stdin: StdinReader,
}

impl Terminal {
	pub(crate) fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, super::Error> {
		let raw_mode = RawMode::new(stdout)?;
		let alternate_screen = AlternateScreen::new(stdout)?;

		Ok(Terminal {
			_raw_mode: raw_mode,
			_alternate_screen: alternate_screen,
			stdin: StdinReader::new()?,
		})
	}

	pub(crate) fn next_event(&mut self, timeout: std::time::Duration) -> Result<Option<u8>, super::Error> {
		Ok(self.stdin.read_timeout(timeout)?)
	}
}

struct RawMode {
	stdout_fd: std::os::unix::io::RawFd,
	original_termios: libc::termios,
}

impl RawMode {
	fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, super::Error> {
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
	fn new(stdout: &mut std::io::StdoutLock<'_>) -> Result<Self, super::Error> {
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

struct StdinReader {
	inner: std::mem::ManuallyDrop<std::fs::File>,
	epoll_fd: std::os::unix::io::RawFd,
}

impl StdinReader {
	fn new() -> Result<Self, super::Error> {
		unsafe {
			let epoll_fd = libc::epoll_create1(libc::EPOLL_CLOEXEC);
			if epoll_fd == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			let stdin = std::io::stdin();
			let stdin_fd = std::os::unix::io::AsRawFd::as_raw_fd(&stdin);
			let mut epoll_event = libc::epoll_event {
				events: libc::EPOLLIN as _,
				u64: 0,
			};
			let result = libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, stdin_fd, &mut epoll_event);
			if result == -1 {
				return Err(std::io::Error::last_os_error().into());
			}

			let inner = std::os::unix::io::FromRawFd::from_raw_fd(stdin_fd);

			Ok(StdinReader {
				inner: std::mem::ManuallyDrop::new(inner),
				epoll_fd,
			})
		}
	}

	fn read_timeout(&mut self, timeout: std::time::Duration) -> std::io::Result<Option<u8>> {
		let result = unsafe {
			let mut epoll_event = libc::epoll_event {
				events: 0,
				u64: 0,
			};
			libc::epoll_wait(
				self.epoll_fd,
				&mut epoll_event,
				1,
				std::convert::TryInto::try_into(timeout.as_millis())
					.map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "timeout is larger than i32::max_value() milliseconds"))?,
			)
		};
		match result {
			1 => {
				let mut buf = [0_u8; 1];
				std::io::Read::read_exact(&mut *self.inner, &mut buf)?;
				Ok(Some(buf[0]))
			},
			0 => Ok(None),
			-1 => Err(std::io::Error::last_os_error()),
			_ => unreachable!(),
		}
	}
}
