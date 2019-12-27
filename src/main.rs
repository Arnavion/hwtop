#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
	clippy::default_trait_access,
	clippy::shadow_unrelated,
	clippy::too_many_lines,
	clippy::use_self,
)]

mod config;

mod hwmon;

mod terminal;

use std::io::Write;

fn main() -> Result<(), Error> {
	let config: config::Config = {
		let mut path = dirs::config_dir().ok_or("config dir not defined")?;
		path.push("hwtop");
		path.push("hwtop.yaml");
		let f = std::fs::File::open(path)?;
		serde_yaml::from_reader(f)?
	};

	let max_sensor_group_name_width = config.sensors.iter().map(|sensor_group| sensor_group.name.len()).max().unwrap_or_default();
	let max_num_temp_sensors = config.sensors.iter().map(|sensor_group| sensor_group.temps.len()).max().unwrap_or_default();

	let sys_devices_system_cpu_present_line_regex = regex::bytes::Regex::new(r"^0-(?P<high>\d+)$")?;
	let proc_cpu_info_line_regex = regex::bytes::Regex::new(r"^(?:(?:processor\s*:\s*(?P<id>\d+))|(?:cpu MHz\s*:\s*(?P<frequency>\d+(?:\.\d+)?)))$")?;

	let stdout = std::io::stdout();
	let mut stdout = stdout.lock();

	let mut terminal = terminal::Terminal::new(&mut stdout)?;

	let mut output = vec![];

	let mut buf = vec![0_u8; 512];

	let num_cpus = hwmon::num_cpus(&sys_devices_system_cpu_present_line_regex, &mut buf)?;

	let mut previous_average_cpu: hwmon::Cpu = Default::default();
	let mut previous_cpus: Box<[hwmon::Cpu]> = vec![Default::default(); num_cpus].into_boxed_slice();

	let mut average_cpu = previous_average_cpu;
	let mut cpus: Box<[(hwmon::Cpu, f32)]> = vec![(Default::default(), 0.); num_cpus].into_boxed_slice();

	let mut previous_networks = vec![hwmon::Network { now: std::time::Instant::now(), rx: 0, tx: 0 }; config.networks.len()].into_boxed_slice();
	let mut networks = previous_networks.clone();

	stdout.write_all(b"\x1B[2J\x1B[3J")?;

	interval(config.interval, &mut terminal, |event| {
		output.clear();

		let show_sensor_names = match event {
			Some(b'i') => true,
			Some(b'q') | Some(b'\x1B') => return Ok(true),
			_ => false,
		};

		if config.cpus.use_sysfs {
			for (id, cpu) in cpus.iter_mut().enumerate() {
				hwmon::parse_scaling_cur_freq(id, &mut cpu.1, &mut buf)?;
			}
		}
		else {
			hwmon::parse_proc_cpuinfo(
				&mut cpus,
				&proc_cpu_info_line_regex,
				&mut buf,
			)?;
		}

		hwmon::parse_proc_stat(
			&mut average_cpu,
			&mut cpus,
			&mut buf,
		)?;

		for (network_spec, network) in config.networks.iter().zip(networks.iter_mut()) {
			network.update(&network_spec.rx_path, &network_spec.tx_path, &mut buf)?;
		}

		output.write_all(b"\x1B[1;1H")?;

		let num_rows = (num_cpus + config.cpus.cols - 1) / config.cpus.cols;
		for row in 0..num_rows {
			for col in 0..(config.cpus.cols) {
				if col > 0 {
					output.write_all(b"  ")?;
				}
				let id = row + num_rows * col;
				if let Some((cpu, frequency)) = cpus.get(id) {
					let previous_cpu = previous_cpus.get_mut(id).unwrap();
					print_cpu(&mut output, Some((id, *frequency)), *cpu, *previous_cpu)?;
					*previous_cpu = *cpu;
				}
			}

			output.write_all(b"\r\n")?;
		}

		print_cpu(&mut output, None, average_cpu, previous_average_cpu)?;
		previous_average_cpu = average_cpu;

		if !config.sensors.is_empty() {
			output.write_all(b"\r\n")?;

			for sensor_group in &config.sensors {
				output.write_all(b"\r\n")?;

				write!(output, "{:>max_sensor_group_name_width$}", sensor_group.name, max_sensor_group_name_width = max_sensor_group_name_width)?;
				output.write_all(b": ")?;

				for (i, sensor) in sensor_group.temps.iter().enumerate() {
					if i > 0 {
						output.write_all(b"  ")?;
					}

					print_temp_sensor(&mut output, &sensor, show_sensor_names, &mut buf)?;
				}

				if !sensor_group.fans.is_empty() {
					for _ in 0..(max_num_temp_sensors - sensor_group.temps.len()) {
						output.write_all(b"         ")?;
					}

					for sensor in &sensor_group.fans {
						output.write_all(b"  ")?;
						print_fan_sensor(&mut output, &sensor, show_sensor_names, &mut buf)?;
					}
				}
			}
		}

		if !config.networks.is_empty() {
			output.write_all(b"\r\n")?;

			for ((network_spec, &network), previous_network) in config.networks.iter().zip(networks.iter()).zip(previous_networks.iter_mut()) {
				output.write_all(b"\r\n")?;
				print_network(&mut output, &network_spec.name, network, *previous_network)?;
				*previous_network = network;
			}
		}

		output.write_all(b"    Press i to show sensor names, q to exit")?;

		stdout.write_all(&output)?;
		stdout.flush()?;

		Ok(false)
	})?;

	Ok(())
}

fn interval(
	interval: std::time::Duration,
	terminal: &mut terminal::Terminal,
	mut f: impl FnMut(Option<u8>) -> Result<bool, Error>,
) -> Result<(), Error> {
	let mut event = None;

	loop {
		let iteration_start = std::time::Instant::now();

		if f(event)? {
			break;
		}

		let iteration_end = std::time::Instant::now();
		let sleep_duration = (iteration_start + interval) - iteration_end;

		event = terminal.next_event(sleep_duration)?;
	}

	Ok(())
}

fn print_cpu<W>(mut writer: W, id_and_frequency: Option<(usize, f32)>, cpu: hwmon::Cpu, previous: hwmon::Cpu) -> Result<(), Error> where W: Write {
	let diff_total = cpu.total - previous.total;
	let diff_used = cpu.used - previous.used;
	#[allow(clippy::cast_precision_loss)]
	let usage = if diff_total == 0 { 0. } else { (100 * diff_used) as f32 / diff_total as f32 };

	let color = match usage {
		usage if usage < 5. => b"0;34",
		usage if usage < 10. => b"1;34",
		usage if usage < 25. => b"1;32",
		usage if usage < 50. => b"1;33",
		usage if usage < 75. => b"0;33",
		usage if usage < 90. => b"1;31",
		_ => b"0;31",
	};

	writer.write_all(b"\x1B[")?;
	writer.write_all(color)?;
	writer.write_all(b"m")?;

	if let Some((id, _)) = id_and_frequency {
		write!(writer, "{:3}", id)?;
		writer.write_all(b": ")?;
	}
	else {
		writer.write_all(b"Avg: ")?;
	}

	write!(writer, "{:5.1}", usage)?;
	writer.write_all(b"% ")?;

	if let Some((_, frequency)) = id_and_frequency {
		if frequency < 999.95 {
			write!(writer, "{:5.1}", frequency)?;
			writer.write_all(b" MHz")?;
		}
		else {
			write!(writer, "{:5.3}", frequency / 1000.)?;
			writer.write_all(b" GHz")?;
		}
	}

	writer.write_all(b"\x1B[0m")?;

	Ok(())
}

fn print_network<W>(mut writer: W, name: &str, network: hwmon::Network, previous: hwmon::Network) -> Result<(), Error> where W: Write {
	writer.write_all(name.as_bytes())?;
	writer.write_all(b": ")?;

	if previous.rx == 0 && previous.tx == 0 {
		return Ok(());
	}

	#[allow(clippy::cast_precision_loss)]
	let rx_speed = (network.rx - previous.rx) as f64 / (network.now.duration_since(previous.now).as_millis() as f64 / 1000.) * 8.;
	if rx_speed < 999.5 {
		write!(writer, "{:3.0}", rx_speed)?;
		writer.write_all(b"    b/s down   ")?;
	}
	else if rx_speed < 999_950. {
		write!(writer, "{:5.1}", rx_speed / 1_000.)?;
		writer.write_all(b" Kb/s down   ")?;
	}
	else if rx_speed < 999_950_000. {
		write!(writer, "{:5.1}", rx_speed / 1_000_000.)?;
		writer.write_all(b" Mb/s down   ")?;
	}
	else {
		write!(writer, "{:5.1}", rx_speed / 1_000_000_000.)?;
		writer.write_all(b" Gb/s down   ")?;
	}

	#[allow(clippy::cast_precision_loss)]
	let tx_speed = (network.tx - previous.tx) as f64 / (network.now.duration_since(previous.now).as_millis() as f64 / 1000.) * 8.;
	if tx_speed < 999.5 {
		write!(writer, "{:3.0}", tx_speed)?;
		writer.write_all(b"    b/s up")?;
	}
	else if tx_speed < 999_950. {
		write!(writer, "{:5.1}", tx_speed / 1_000.)?;
		writer.write_all(b" Kb/s up")?;
	}
	else if tx_speed < 999_950_000. {
		write!(writer, "{:5.1}", tx_speed / 1_000_000.)?;
		writer.write_all(b" Mb/s up")?;
	}
	else {
		write!(writer, "{:5.1}", tx_speed / 1_000_000_000.)?;
		writer.write_all(b" Gb/s up")?;
	}

	Ok(())
}

fn print_temp_sensor<W>(mut writer: W, sensor: &config::TempSensor, show_sensor_names: bool, buf: &mut Vec<u8>) -> Result<(), Error> where W: Write {
	let temp = hwmon::parse_temp_sensor(&sensor.path, buf)?.map(|temp| temp + sensor.offset);

	let color = match temp {
		Some(temp) if temp < 30. => &b"0;34"[..],
		Some(temp) if temp < 35. => &b"1;34"[..],
		Some(temp) if temp < 40. => &b"1;32"[..],
		Some(temp) if temp < 45. => &b"1;33"[..],
		Some(temp) if temp < 55. => &b"0;33"[..],
		Some(temp) if temp < 65. => &b"1;31"[..],
		Some(_) => &b"0;31"[..],
		None => &b"0"[..],
	};

	writer.write_all(b"\x1B[")?;
	writer.write_all(color)?;
	writer.write_all(b"m")?;

	match (&sensor.name, temp) {
		(Some(name), _) if show_sensor_names =>
			if name.len() > 7 {
				writer.write_all(name[..6].as_bytes())?;
				writer.write_all(b"\xE2\x80\xA6")?;
			}
			else {
				write!(writer, "{:^7}", name)?;
			},
		(_, Some(temp)) => {
			write!(writer, "{:5.1}", temp)?;
			writer.write_all(b"\xC2\xB0C")?;
		},
		(_, None) => {
			writer.write_all(b"  N/A  ")?;
		},
	}

	writer.write_all(b"\x1B[0m")?;

	Ok(())
}

fn print_fan_sensor<W>(mut writer: W, sensor: &config::FanSensor, show_sensor_names: bool, buf: &mut Vec<u8>) -> Result<(), Error> where W: Write {
	match &sensor.name {
		Some(name) if show_sensor_names =>
			if name.len() > 15 {
				writer.write_all(name[..14].as_bytes())?;
				writer.write_all(b"\xE2\x80\xA6")?;
			}
			else {
				write!(writer, "{:^15}", name)?;
			},
		_ => {
			let fan = hwmon::parse_fan_sensor(&sensor.fan_path, buf)?;
			let pwm = hwmon::parse_pwm_sensor(&sensor.pwm_path, buf)?.map(|pwm| 100. * f32::from(pwm) / 255.);
			if let (Some(fan), Some(pwm)) = (fan, pwm) {
				write!(writer, "{:3.0}", pwm)?;
				writer.write_all(b"% (")?;
				write!(writer, "{:4}", fan)?;
				writer.write_all(b" RPM)")?;
			}
			else {
				writer.write_all(b"      N/A      ")?;
			}
		},
	}

	Ok(())
}

struct Error {
	inner: Box<dyn std::error::Error>,
	#[cfg(debug_assertions)]
	backtrace: backtrace::Backtrace,
}

impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		std::fmt::Display::fmt(self, f)
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		writeln!(f, "{}", self.inner)?;
		#[cfg(debug_assertions)]
		{
			writeln!(f)?;
			writeln!(f, "{:?}", self.backtrace)?;
		}
		Ok(())
	}
}

impl<E> From<E> for Error where E: Into<Box<dyn std::error::Error>> {
	fn from(err: E) -> Self {
		Error {
			inner: err.into(),
			#[cfg(debug_assertions)]
			backtrace: Default::default(),
		}
	}
}
