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

fn main() -> Result<(), Error> {
	let config: config::Config = {
		let f = std::fs::File::open("/etc/sensord/config.yaml")?;
		serde_yaml::from_reader(f)?
	};

	let connection =
		dbus_pure::conn::Connection::new(
			dbus_pure::conn::BusPath::System,
			dbus_pure::conn::SaslAuthType::Uid,
		)?;
	let mut dbus_client = dbus_pure::client::Client::new(connection)?;

	let request_name_result = {
		let body =
			dbus_client.method_call(
				"org.freedesktop.DBus",
				dbus_pure::types::ObjectPath("/org/freedesktop/DBus".into()),
				"org.freedesktop.DBus",
				"RequestName",
				Some(&dbus_pure::types::Variant::Tuple {
					elements: (&[
						dbus_pure::types::Variant::String("dev.arnavion.sensord.Daemon".into()),
						dbus_pure::types::Variant::U32(4), // DBUS_NAME_FLAG_DO_NOT_QUEUE
					][..]).into()
				}),
			)?
			.ok_or("RequestName response has no body")?;
		let body: u32 = serde::Deserialize::deserialize(body)?;
		body
	};
	if request_name_result != 1 {
		return Err(format!("RequestName returned {:?}", request_name_result).into());
	}

	dbus_client.set_name("dev.arnavion.sensord.Daemon".to_owned());

	let sys_devices_system_cpu_present_line_regex = regex::bytes::Regex::new(r"^0-(?P<high>\d+)$")?;
	let proc_cpu_info_line_regex = regex::bytes::Regex::new(r"^(?:(?:processor\s*:\s*(?P<id>\d+))|(?:cpu MHz\s*:\s*(?P<frequency>\d+(?:\.\d+)?)))$")?;

	let mut buf = vec![0_u8; 512];

	let num_cpus = hwmon::num_cpus(&sys_devices_system_cpu_present_line_regex, &mut buf)?;

	let mut previous_average_cpu: hwmon::Cpu = Default::default();
	let mut previous_cpus: Box<[hwmon::Cpu]> = vec![Default::default(); num_cpus].into_boxed_slice();

	let mut average_cpu = previous_average_cpu;
	let mut cpus: Box<[(hwmon::Cpu, f64)]> = vec![(Default::default(), 0.); num_cpus].into_boxed_slice();

	let num_cpus: u32 = std::convert::TryInto::try_into(num_cpus)?;

	let mut previous_networks = vec![hwmon::Network { now: std::time::Instant::now(), rx: 0, tx: 0 }; config.networks.len()].into_boxed_slice();
	let mut networks = previous_networks.clone();

	interval(config.interval, || {
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

		let cpus: Vec<_> =
			previous_cpus.iter_mut()
			.zip(&*cpus)
			.map(|(previous_cpu, &(cpu, frequency))| {
				let diff_total = cpu.total - previous_cpu.total;
				let diff_used = cpu.used - previous_cpu.used;

				*previous_cpu = cpu;

				#[allow(clippy::cast_precision_loss)]
				let usage = if diff_total == 0 { 0. } else { (100 * diff_used) as f64 / diff_total as f64 };
				(usage, frequency)
			})
			.map(|(usage, frequency)| dbus_pure::types::Variant::Struct {
				fields: vec![
					dbus_pure::types::Variant::F64(usage),
					dbus_pure::types::Variant::F64(frequency),
				].into(),
			})
			.collect();
		let cpus = dbus_pure::types::Variant::Array {
			element_signature: dbus_pure::types::Signature::Struct {
				fields: vec![
					dbus_pure::types::Signature::F64,
					dbus_pure::types::Signature::F64,
				],
			},
			elements: cpus.into(),
		};

		let average_cpu = dbus_pure::types::Variant::F64({
			let diff_total = average_cpu.total - previous_average_cpu.total;
			let diff_used = average_cpu.used - previous_average_cpu.used;

			previous_average_cpu = average_cpu;

			#[allow(clippy::cast_precision_loss)]
			let usage = if diff_total == 0 { 0. } else { (100 * diff_used) as f64 / diff_total as f64 };
			usage
		});

		let sensors: Result<Vec<_>, Error> =
			config.sensors.iter()
			.map(|sensor_group| Ok(dbus_pure::types::Variant::Struct {
				fields: vec![
					dbus_pure::types::Variant::String((&*sensor_group.name).into()),
					dbus_pure::types::Variant::Array {
						element_signature: dbus_pure::types::Signature::Struct {
							fields: vec![
								dbus_pure::types::Signature::String,
								dbus_pure::types::Signature::F64,
							],
						},
						elements: {
							let temp_sensors: Result<Vec<_>, Error> =
								sensor_group.temps.iter()
								.map(|sensor| {
									let temp = hwmon::parse_temp_sensor(&sensor.path, &mut buf)?.map(|temp| temp + sensor.offset);
									Ok(dbus_pure::types::Variant::Struct {
										fields: vec![
											dbus_pure::types::Variant::String(sensor.name.as_ref().map_or("", AsRef::as_ref).into()),
											dbus_pure::types::Variant::F64(temp.unwrap_or_default()),
										].into(),
									})
								})
								.collect();
							temp_sensors?.into()
						},
					},
					dbus_pure::types::Variant::Array {
						element_signature: dbus_pure::types::Signature::Struct {
							fields: vec![
								dbus_pure::types::Signature::String,
								dbus_pure::types::Signature::U32,
								dbus_pure::types::Signature::U8,
							],
						},
						elements: {
							let fan_sensors: Result<Vec<_>, Error> =
								sensor_group.fans.iter()
								.map(|sensor| {
									let fan = hwmon::parse_fan_sensor(&sensor.fan_path, &mut buf)?;
									let pwm = hwmon::parse_pwm_sensor(&sensor.pwm_path, &mut buf)?;
									Ok(dbus_pure::types::Variant::Struct {
										fields: vec![
											dbus_pure::types::Variant::String(sensor.name.as_ref().map_or("", AsRef::as_ref).into()),
											dbus_pure::types::Variant::U32(fan.unwrap_or_default()),
											dbus_pure::types::Variant::U8(pwm.unwrap_or_default()),
										].into(),
									})
								})
								.collect();
							fan_sensors?.into()
						},
					},
				].into(),
			}))
			.collect();
		let sensors = sensors?;
		let sensors = dbus_pure::types::Variant::Array {
			element_signature: dbus_pure::types::Signature::Struct {
				fields: vec![
					dbus_pure::types::Signature::String,
					dbus_pure::types::Signature::Array {
						element: Box::new(dbus_pure::types::Signature::Struct {
							fields: vec![
								dbus_pure::types::Signature::String,
								dbus_pure::types::Signature::F64,
							],
						}),
					},
					dbus_pure::types::Signature::Array {
						element: Box::new(dbus_pure::types::Signature::Struct {
							fields: vec![
								dbus_pure::types::Signature::String,
								dbus_pure::types::Signature::U32,
								dbus_pure::types::Signature::U8,
							],
						}),
					},
				],
			},
			elements: sensors.into(),
		};

		let networks: Vec<_> =
			config.networks.iter()
			.zip(&*networks)
			.zip(&mut *previous_networks)
			.map(|((network_spec, &network), previous_network)| {
				let (rx, tx) =
					if previous_network.rx == 0 && previous_network.tx == 0 {
						(0., 0.)
					}
					else {
						#[allow(clippy::cast_precision_loss)]
						let rx =
							(network.rx - previous_network.rx) as f64 /
							(network.now.duration_since(previous_network.now).as_millis() as f64 / 1000.);
						#[allow(clippy::cast_precision_loss)]
						let tx =
							(network.tx - previous_network.tx) as f64 /
							(network.now.duration_since(previous_network.now).as_millis() as f64 / 1000.);
						(rx, tx)
					};

				*previous_network = network;

				dbus_pure::types::Variant::Struct {
					fields: vec![
						dbus_pure::types::Variant::String((&*network_spec.name).into()),
						dbus_pure::types::Variant::F64(rx),
						dbus_pure::types::Variant::F64(tx),
					].into(),
				}
			})
			.collect();
		let networks = dbus_pure::types::Variant::Array {
			element_signature: dbus_pure::types::Signature::Struct {
				fields: vec![
					dbus_pure::types::Signature::String,
					dbus_pure::types::Signature::F64,
					dbus_pure::types::Signature::F64,
				],
			},
			elements: networks.into(),
		};

		let body = dbus_pure::types::Variant::Struct {
			fields: vec![
				dbus_pure::types::Variant::U32(num_cpus),
				cpus,
				average_cpu,
				sensors,
				networks,
			].into(),
		};

		let _ = dbus_client.send(
			&mut dbus_pure::types::MessageHeader {
				r#type: dbus_pure::types::MessageType::Signal {
					interface: "dev.arnavion.sensord.Daemon".into(),
					member: "Sensors".into(),
					path: dbus_pure::types::ObjectPath("/dev/arnavion/sensord/Daemon".into()),
				},
				flags: dbus_pure::types::message_flags::NO_REPLY_EXPECTED,
				body_len: 0,
				serial: 0,
				fields: (&[][..]).into(),
			},
			Some(&body),
		)?;

		Ok(false)
	})?;

	Ok(())
}

fn interval(
	interval: std::time::Duration,
	mut f: impl FnMut() -> Result<bool, Error>,
) -> Result<(), Error> {
	loop {
		let iteration_start = std::time::Instant::now();

		if f()? {
			break;
		}

		let iteration_end = std::time::Instant::now();
		let sleep_duration = (iteration_start + interval) - iteration_end;

		std::thread::sleep(sleep_duration);
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

		let mut source = self.inner.source();
		while let Some(err) = source {
			writeln!(f, "caused by: {}", err)?;
			source = err.source();
		}

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
