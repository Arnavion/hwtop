#[derive(Clone, Copy, Default)]
pub(crate) struct Cpu {
	pub(crate) total: u64,
	pub(crate) used: u64,
}

pub(crate) fn num_cpus(sys_devices_system_cpu_present_line_regex: &regex::bytes::Regex, buf: &mut Vec<u8>) -> Result<usize, super::Error> {
	let mut result: Option<usize> = None;

	for_each_line("/sys/devices/system/cpu/present", buf, |line| {
		let captures = sys_devices_system_cpu_present_line_regex.captures(line).ok_or("could not parse /sys/devices/system/cpu/present")?;
		let high = captures.name("high").unwrap();
		let high = std::str::from_utf8(high.as_bytes())?;
		let high: usize = high.parse()?;
		result = Some(high + 1);
		Ok(true)
	})?;

	Ok(result.ok_or("/sys/devices/system/cpu/present is empty")?)
}

pub(crate) fn parse_proc_cpuinfo(cpus: &mut [(Cpu, f64)], proc_cpu_info_line_regex: &regex::bytes::Regex, buf: &mut Vec<u8>) -> Result<(), super::Error> {
	let mut current_id: Option<usize> = None;

	for_each_line("/proc/cpuinfo", buf, |line| {
		if line.is_empty() {
			current_id = None;
		}
		else if let Some(captures) = proc_cpu_info_line_regex.captures(line) {
			if let Some(id) = captures.name("id") {
				let id = std::str::from_utf8(id.as_bytes())?;
				current_id = Some(id.parse()?);
			}
			else if let Some(frequency) = captures.name("frequency") {
				let frequency = std::str::from_utf8(frequency.as_bytes())?;
				let id = current_id.ok_or("unexpected `cpu MHz` line without corresponding `processor` line")?;
				cpus.get_mut(id).ok_or_else(|| format!("unexpected CPU ID {}", id))?.1 = frequency.parse()?;
			}
		}

		Ok(false)
	})
}

pub(crate) fn parse_proc_stat(average_cpu: &mut Cpu, cpus: &mut [(Cpu, f64)], buf: &mut Vec<u8>) -> Result<(), super::Error> {
	for_each_line("/proc/stat", buf, |line| {
		if !line.starts_with(b"cpu") {
			return Ok(true);
		}

		let mut parts = line.split(|&b| b == b' ').filter(|s| !s.is_empty());

		let id = parts.next().unwrap();
		let id = &id[(b"cpu".len())..];
		let id: Option<usize> =
			if id.is_empty() {
				None
			}
			else {
				let id = std::str::from_utf8(id)?;
				let id = id.parse()?;
				Some(id)
			};

		let cpu =
			if let Some(id) = id {
				&mut cpus.get_mut(id).ok_or_else(|| format!("unexpected CPU ID {}", id))?.0
			}
			else {
				&mut *average_cpu
			};

		let mut parts =
			parts
			.map(|part| -> Result<u64, super::Error> {
				let part = std::str::from_utf8(part)?;
				let part = part.parse()?;
				Ok(part)
			})
			.fuse();

		let user_time = parts.next().ok_or("user time missing")??;
		let nice_time = parts.next().ok_or("nice time missing")??;
		let system_time = parts.next().ok_or("system time missing")??;
		let idle_time = parts.next().ok_or("idle time missing")??;
		let iowait_time = parts.next().unwrap_or(Ok(0))?;
		let irq_time = parts.next().unwrap_or(Ok(0))?;
		let softirq_time = parts.next().unwrap_or(Ok(0))?;

		cpu.total = user_time + nice_time + system_time + idle_time + iowait_time + irq_time + softirq_time;
		cpu.used = user_time + nice_time + system_time + irq_time + softirq_time;

		Ok(false)
	})
}

pub(crate) fn parse_scaling_cur_freq(id: usize, cpu_freq: &mut f64, buf: &mut Vec<u8>) -> Result<(), super::Error> {
	*cpu_freq = parse_hwmon::<f64>(std::path::Path::new(&format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", id)), buf)?.unwrap_or_default() / 1000.;

	Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct Network {
	pub(crate) now: std::time::Instant,
	pub(crate) rx: u64,
	pub(crate) tx: u64,
}

impl Network {
	pub(crate) fn update(&mut self, rx_path: &std::path::Path, tx_path: &std::path::Path, buf: &mut Vec<u8>) -> Result<(), super::Error> {
		self.now = std::time::Instant::now();
		self.rx = parse_hwmon(rx_path, buf)?.unwrap();
		self.tx = parse_hwmon(tx_path, buf)?.unwrap();
		Ok(())
	}
}

pub(crate) fn parse_temp_sensor(path: Option<&std::path::Path>, buf: &mut Vec<u8>) -> Result<Option<f64>, super::Error> {
	match path {
		Some(path) => match parse_hwmon::<f64>(path, buf) {
			Ok(Some(temp)) => Ok(Some(temp / 1000.)),
			result => result,
		},
		None => Ok(None),
	}
}

pub(crate) fn parse_fan_sensor(path: Option<&std::path::Path>, buf: &mut Vec<u8>) -> Result<Option<u16>, super::Error> {
	path.map_or(Ok(None), |path| parse_hwmon(path, buf))
}

pub(crate) fn parse_pwm_sensor(path: Option<&std::path::Path>, buf: &mut Vec<u8>) -> Result<Option<u8>, super::Error> {
	path.map_or(Ok(None), |path| parse_hwmon(path, buf))
}

fn for_each_line(
	path: impl AsRef<std::path::Path>,
	buf: &mut Vec<u8>,
	mut f: impl FnMut(&[u8]) -> Result<bool, super::Error>,
) -> Result<(), super::Error> {
	let mut file = std::io::BufReader::new(std::fs::File::open(path.as_ref())?);

	loop {
		buf.clear();

		let read = std::io::BufRead::read_until(&mut file, b'\n', buf)?;
		if read == 0 {
			break;
		}

		let buf = &buf[..read];
		let buf =
			if buf[buf.len() - 1] == b'\n' {
				&buf[..(buf.len() - 1)]
			}
			else {
				buf
			};

		if f(buf)? {
			break;
		}
	}

	Ok(())
}

fn parse_hwmon<T>(path: &std::path::Path, buf: &mut Vec<u8>) -> Result<Option<T>, super::Error>
where
	T: std::str::FromStr,
	super::Error: From<<T as std::str::FromStr>::Err>,
{
	let file = match std::fs::File::open(path) {
		Ok(file) => file,
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(err) => return Err(err.into()),
	};
	let mut file = std::io::BufReader::new(file);

	buf.clear();

	let read = match std::io::BufRead::read_until(&mut file, b'\n', buf) {
		Ok(0) => return Err("file is empty".into()),
		Ok(read) => read,
		Err(err) if err.raw_os_error() == Some(libc::ENXIO) => return Ok(None),
		Err(err) => return Err(err.into()),
	};
	let buf = &buf[..read];
	let buf =
		if buf[buf.len() - 1] == b'\n' {
			&buf[..(buf.len() - 1)]
		}
		else {
			buf
		};

	let value = std::str::from_utf8(buf)?;
	let value: T = value.parse()?;

	Ok(Some(value))
}
