#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
	clippy::let_unit_value,
	clippy::too_many_lines,
)]

use std::io::Write;

fn main() -> Result<(), Error> {
	let connection =
		dbus_pure::Connection::new(
			dbus_pure::BusPath::System,
			dbus_pure::SaslAuthType::Uid,
		)?;
	let mut dbus_client = dbus_pure::Client::new(connection)?;

	{
		let obj = OrgFreeDesktopDbusObject {
			name: "org.freedesktop.DBus".into(),
			path: dbus_pure::proto::ObjectPath("/org/freedesktop/DBus".into()),
		};
		let () =
			obj.add_match(
				&mut dbus_client,
				"type='signal',path='/dev/arnavion/sensord/Daemon',interface='dev.arnavion.sensord.Daemon',member='Sensors'",
			)?;
	}

	let (event_sender, event_receiver) = std::sync::mpsc::channel();

	std::thread::spawn({
		let event_sender = event_sender.clone();

		move || {
			loop {
				match dbus_client.recv() {
					Ok((header, body)) => match header.r#type {
						dbus_pure::proto::MessageType::Signal { interface, member, path: _ } if interface == "dev.arnavion.sensord.Daemon" && member == "Sensors" => {
							if event_sender.send(Ok(Event::Sensors(body))).is_err() {
								break;
							}
						},
						_ => (),
					},

					Err(err) => {
						let _ = event_sender.send(Err(Error::from(err)));
						break;
					},
				}
			}
		}
	});

	std::thread::spawn(move || {
		let mut stdin = std::io::stdin().lock();

		loop {
			let mut b = 0_u8;
			match std::io::Read::read_exact(&mut stdin, std::slice::from_mut(&mut b)) {
				Ok(()) => if event_sender.send(Ok(Event::Stdin(b))).is_err() {
					break;
				},
				Err(err) => {
					let _ = event_sender.send(Err(Error::from(err)));
					break;
				},
			}
		}
	});

	let stdout = std::io::stdout().lock();
	let stdout = terminal::RawMode::new(stdout)?;
	let mut terminfo = terminal::terminfo::Terminfo::from_env()?;
	let stdout = terminal::VtMode::new(stdout, terminfo.alternate_screen())?;
	let stdout = terminal::VtMode::new(stdout, terminfo.hide_cursor())?;
	let mut stdout = terminal::VtMode::new(stdout, terminfo.no_wraparound())?;

	let mut output = vec![];

	let mut message = loop {
		match event_receiver.recv()?? {
			Event::Sensors(new_message) => {
				let new_message = new_message.ok_or("signal has no body")?;
				let new_message: sensord_common::SensorsMessage<'static> = serde::Deserialize::deserialize(new_message)?;
				break new_message;
			},

			Event::Stdin(_) => (),
		}
	};

	let mut show_sensor_names = false;

	loop {
		show_sensor_names = match event_receiver.recv()?? {
			Event::Sensors(new_message) => {
				let new_message = new_message.ok_or("signal has no body")?;
				let new_message: sensord_common::SensorsMessage<'static> = serde::Deserialize::deserialize(new_message)?;
				message = new_message;
				show_sensor_names
			},

			Event::Stdin(b's') => !show_sensor_names,

			Event::Stdin(b'q' | b'\x1B') => break,

			Event::Stdin(_) => show_sensor_names,
		};

		let max_sensor_group_name_width = message.sensors.iter().map(|sensor_group| sensor_group.name.len()).max().unwrap_or_default();
		let max_num_temp_sensors = message.sensors.iter().map(|sensor_group| sensor_group.temps.len()).max().unwrap_or_default();
		let max_network_name_width = message.networks.iter().map(|network| network.name.len()).max().unwrap_or_default();

		let num_cpus = message.cpus.len();

		output.clear();


		let (begin_sync, _) = terminfo.sync()?;
		output.write_all(begin_sync)?;
		output.write_all(terminfo.clear_screen())?;
		output.write_all(terminfo.clear_scrollback())?;

		let terminal_width: usize = terminal::Terminal::width(&stdout)?;
		let num_cpu_cols = terminal_width.saturating_sub(21) / 23 + 1;

		let num_rows = num_cpus.div_ceil(num_cpu_cols);
		for row in 0..num_rows {
			for col in 0..num_cpu_cols {
				if col > 0 {
					output.write_all(b"  ")?;
				}
				let id = row + num_rows * col;
				if let Some(cpu) = message.cpus.get(id) {
					print_cpu(&mut output, Some((id, cpu.frequency)), cpu.usage)?;
				}
			}

			output.write_all(b"\r\n")?;
		}

		print_cpu(&mut output, None, message.cpu_average_usage)?;

		if !message.sensors.is_empty() {
			output.write_all(b"\r\n")?;

			for sensor_group in &*message.sensors {
				output.write_all(b"\r\n")?;

				write!(output, "{:>max_sensor_group_name_width$}", sensor_group.name)?;
				output.write_all(b": ")?;

				for (i, sensor) in sensor_group.temps.iter().enumerate() {
					if i > 0 {
						output.write_all(b"  ")?;
					}

					print_temp_sensor(&mut output, sensor, show_sensor_names)?;
				}

				if !sensor_group.fans.is_empty() {
					for _ in 0..(max_num_temp_sensors - sensor_group.temps.len()) {
						output.write_all(b"         ")?;
					}

					for sensor in &sensor_group.fans {
						output.write_all(b"  ")?;
						print_fan_sensor(&mut output, sensor, show_sensor_names)?;
					}
				}

				if !sensor_group.bats.is_empty() {
					for _ in 0..(max_num_temp_sensors - sensor_group.temps.len()) {
						output.write_all(b"         ")?;
					}

					for sensor in &sensor_group.bats {
						output.write_all(b"  ")?;
						print_bat_sensor(&mut output, sensor, show_sensor_names)?;
					}
				}
			}
		}

		if !message.networks.is_empty() {
			output.write_all(b"\r\n")?;

			for network in &*message.networks {
				output.write_all(b"\r\n")?;
				print_network(&mut output, network, max_network_name_width, show_sensor_names)?;
			}
		}

		output.write_all(b"  [s]ensor names  [q]uit")?;

		let (_, end_sync) = terminfo.sync()?;
		output.write_all(end_sync)?;

		stdout.write_all(&output)?;
		stdout.flush()?;
	}

	Ok(())
}

#[derive(Debug)]
enum Event {
	Sensors(Option<dbus_pure::proto::Variant<'static>>),
	Stdin(u8),
}

fn print_cpu<W>(mut writer: W, id_and_frequency: Option<(usize, f64)>, usage: f64) -> Result<(), Error> where W: Write {
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
		write!(writer, "{id:3}")?;
		writer.write_all(b": ")?;
	}
	else {
		writer.write_all(b"Avg: ")?;
	}

	write!(writer, "{usage:5.1}")?;
	writer.write_all(b"% ")?;

	if let Some((_, frequency)) = id_and_frequency {
		if frequency < 999.95 {
			write!(writer, "{frequency:5.1}")?;
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

fn print_temp_sensor<W>(mut writer: W, sensor: &sensord_common::TempSensor<'_>, show_sensor_names: bool) -> Result<(), Error> where W: Write {
	let temp = sensor.value;

	let color = match temp {
		0. => &b"0"[..],
		temp if temp < 30. => &b"0;34"[..],
		temp if temp < 35. => &b"1;34"[..],
		temp if temp < 40. => &b"1;32"[..],
		temp if temp < 45. => &b"1;33"[..],
		temp if temp < 55. => &b"0;33"[..],
		temp if temp < 65. => &b"1;31"[..],
		_ => &b"0;31"[..],
	};

	writer.write_all(b"\x1B[")?;
	writer.write_all(color)?;
	writer.write_all(b"m")?;

	match (&sensor.name, temp) {
		(name, _) if show_sensor_names =>
			if name.len() > 7 {
				writer.write_all(name[..6].as_bytes())?;
				writer.write_all(b"\xE2\x80\xA6")?;
			}
			else {
				write!(writer, "{name:^7}")?;
			},

		(_, temp) if temp > 0. => {
			write!(writer, "{temp:5.1}")?;
			writer.write_all(b"\xC2\xB0C")?;
		},

		(_, _) => {
			writer.write_all(b"  N/A  ")?;
		},
	}

	writer.write_all(b"\x1B[0m")?;

	Ok(())
}

fn print_fan_sensor<W>(mut writer: W, sensor: &sensord_common::FanSensor<'_>, show_sensor_names: bool) -> Result<(), Error> where W: Write {
	match &sensor.name {
		name if show_sensor_names =>
			if name.len() > 15 {
				writer.write_all(name[..14].as_bytes())?;
				writer.write_all(b"\xE2\x80\xA6")?;
			}
			else {
				write!(writer, "{name:^15}")?;
			},
		_ => {
			let pwm = 100. * f64::from(sensor.pwm) / 255.;
			write!(writer, "{pwm:3.0}")?;
			writer.write_all(b"% (")?;
			write!(writer, "{:4}", sensor.fan)?;
			writer.write_all(b" RPM)")?;
		},
	}

	Ok(())
}

fn print_bat_sensor<W>(mut writer: W, sensor: &sensord_common::BatSensor<'_>, show_sensor_names: bool) -> Result<(), Error> where W: Write {
	match (&sensor.name, sensor.capacity, sensor.charging) {
		(name, _, _) if show_sensor_names =>
			if name.len() > 15 {
				writer.write_all(name[..14].as_bytes())?;
				writer.write_all(b"\xE2\x80\xA6")?;
			}
			else {
				write!(writer, "{name:^15}")?;
			},

		(_, capacity, charging) if capacity > 0 => {
			writer.write_all(if charging { b"+" } else { b"-" })?;
			write!(writer, "{capacity:4}")?;
			writer.write_all(b"% ")?;
		},

		(_, _, _) => {
			writer.write_all(b"  N/A  ")?;
		},
	}

	Ok(())
}

fn print_network<W>(mut writer: W, network: &sensord_common::Network<'_>, max_network_name_width: usize, show_sensor_names: bool) -> Result<(), Error> where W: Write {
	write!(writer, "{:>max_network_name_width$}", network.name)?;
	writer.write_all(b": ")?;

	if show_sensor_names {
		for address in &network.addresses {
			write!(writer, "{address} ")?;
		}
	}
	else {
		let rx_speed = network.rx * 8.;
		if rx_speed == 0. {
			write!(writer, "\x1B[0;34m{rx_speed:3.0}")?;
			writer.write_all(b"    b/s down\x1B[0m   ")?;
		}
		else if rx_speed < 999.5 {
			write!(writer, "\x1B[1;34m{rx_speed:3.0}")?;
			writer.write_all(b"    b/s down\x1B[0m   ")?;
		}
		else if rx_speed < 999_950. {
			write!(writer, "\x1B[1;32m{:5.1}", rx_speed / 1_000.)?;
			writer.write_all(b" Kb/s down\x1B[0m   ")?;
		}
		else if rx_speed < 999_950_000. {
			write!(writer, "\x1B[1;33m{:5.1}", rx_speed / 1_000_000.)?;
			writer.write_all(b" Mb/s down\x1B[0m   ")?;
		}
		else {
			write!(writer, "\x1B[1;31m{:5.1}", rx_speed / 1_000_000_000.)?;
			writer.write_all(b" Gb/s down\x1B[0m   ")?;
		}

		let tx_speed = network.tx * 8.;
		if tx_speed == 0. {
			write!(writer, "\x1B[0;34m{tx_speed:3.0}")?;
			writer.write_all(b"    b/s up\x1B[0m")?;
		}
		else if tx_speed < 999.5 {
			write!(writer, "\x1B[1;34m{tx_speed:3.0}")?;
			writer.write_all(b"    b/s up\x1B[0m")?;
		}
		else if tx_speed < 999_950. {
			write!(writer, "\x1B[1;32m{:5.1}", tx_speed / 1_000.)?;
			writer.write_all(b" Kb/s up\x1B[0m")?;
		}
		else if tx_speed < 999_950_000. {
			write!(writer, "\x1B[1;33m{:5.1}", tx_speed / 1_000_000.)?;
			writer.write_all(b" Mb/s up\x1B[0m")?;
		}
		else {
			write!(writer, "\x1B[1;31m{:5.1}", tx_speed / 1_000_000_000.)?;
			writer.write_all(b" Gb/s up\x1B[0m")?;
		}
	}

	Ok(())
}

struct Error {
	inner: Box<dyn std::error::Error + Send + Sync>,
}

impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		std::fmt::Display::fmt(self, f)
	}
}

impl std::fmt::Display for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		writeln!(f, "{}", self.inner)?;

		let mut source = self.inner.source();
		while let Some(err) = source {
			writeln!(f, "caused by: {err}")?;
			source = err.source();
		}

		Ok(())
	}
}

impl<E> From<E> for Error where E: Into<Box<dyn std::error::Error + Send + Sync>> {
	fn from(err: E) -> Self {
		Error {
			inner: err.into(),
		}
	}
}

#[dbus_pure_macros::interface("org.freedesktop.DBus")]
trait OrgFreeDesktopDbusInterface {
	#[name = "AddMatch"]
	fn add_match(rule: &str);
}

#[dbus_pure_macros::object(OrgFreeDesktopDbusInterface)]
struct OrgFreeDesktopDbusObject;
