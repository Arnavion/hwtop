#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
	clippy::default_trait_access,
	clippy::let_and_return,
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
		dbus_pure::Connection::new(
			dbus_pure::BusPath::System,
			dbus_pure::SaslAuthType::Uid,
		)?;
	let mut dbus_client = dbus_pure::Client::new(connection)?;

	let request_name_result = {
		let obj = OrgFreeDesktopDbusObject {
			name: "org.freedesktop.DBus".into(),
			path: dbus_pure::proto::ObjectPath("/org/freedesktop/DBus".into()),
		};
		let request_name_result =
			obj.request_name(
				&mut dbus_client,
				"dev.arnavion.sensord.Daemon",
				4, // DBUS_NAME_FLAG_DO_NOT_QUEUE
			)?;
		request_name_result
	};
	if request_name_result != 1 {
		return Err(format!("RequestName returned {request_name_result:?}").into());
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
	let mut message_cpus: Box<[sensord_common::Cpu]> = vec![Default::default(); num_cpus].into_boxed_slice();

	let num_cpus: u32 = num_cpus.try_into()?;

	let mut message_sensor_groups: Box<[sensord_common::SensorGroup<'_>]> =
		config.sensors.iter()
		.map(|sensor_group| sensord_common::SensorGroup {
			name: (&sensor_group.name).into(),
			temps:
				sensor_group.temps.iter()
				.map(|sensor| {
					sensord_common::TempSensor {
						name: sensor.name.as_ref().map_or("", AsRef::as_ref).into(),
						value: 0.,
					}
				})
				.collect(),
			fans:
				sensor_group.fans.iter()
				.map(|sensor| {
					sensord_common::FanSensor {
						name: sensor.name.as_ref().map_or("", AsRef::as_ref).into(),
						fan: 0,
						pwm: 0,
					}
				})
				.collect(),
			bats:
				sensor_group.bats.iter()
				.map(|sensor| {
					sensord_common::BatSensor {
						name: sensor.name.as_ref().map_or("", AsRef::as_ref).into(),
						capacity: 0,
						charging: false,
					}
				})
				.collect(),
		})
		.collect::<Vec<_>>()
		.into_boxed_slice();

	let mut previous_networks = vec![hwmon::Network { now: std::time::Instant::now(), rx: 0, tx: 0 }; config.networks.len()].into_boxed_slice();
	let mut networks = previous_networks.clone();
	let mut message_networks: Box<[sensord_common::Network<'_>]> =
		config.networks.iter()
		.map(|network| sensord_common::Network {
			name: (&network.name).into(),
			rx: 0.,
			tx: 0.,
		})
		.collect::<Vec<_>>()
		.into_boxed_slice();

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

		for ((previous_cpu, &(cpu, frequency)), message_cpu) in previous_cpus.iter_mut().zip(&*cpus).zip(&mut *message_cpus) {
			let diff_total = cpu.total - previous_cpu.total;
			let diff_used = cpu.used - previous_cpu.used;

			*previous_cpu = cpu;

			#[allow(clippy::cast_precision_loss)]
			let usage = if diff_total == 0 { 0. } else { (100 * diff_used) as f64 / diff_total as f64 };

			message_cpu.usage = usage;
			message_cpu.frequency = frequency;
		}

		let cpu_average_usage = {
			let diff_total = average_cpu.total - previous_average_cpu.total;
			let diff_used = average_cpu.used - previous_average_cpu.used;

			previous_average_cpu = average_cpu;

			#[allow(clippy::cast_precision_loss)]
			let usage = if diff_total == 0 { 0. } else { (100 * diff_used) as f64 / diff_total as f64 };
			usage
		};

		for (sensor_group, message_sensor_group) in config.sensors.iter().zip(&mut *message_sensor_groups) {
			for (sensor, message_temp_sensor) in sensor_group.temps.iter().zip(&mut *message_sensor_group.temps) {
				let temp = hwmon::parse_temp_sensor(sensor.path.as_deref(), &mut buf)?.map(|temp| temp + sensor.offset);
				message_temp_sensor.value = temp.unwrap_or_default();
			}

			for (sensor, message_fan_sensor) in sensor_group.fans.iter().zip(&mut *message_sensor_group.fans) {
				let fan = hwmon::parse_fan_sensor(sensor.fan_path.as_deref(), &mut buf)?;
				let pwm = hwmon::parse_pwm_sensor(sensor.pwm_path.as_deref(), &mut buf)?;
				message_fan_sensor.fan = fan.unwrap_or_default();
				message_fan_sensor.pwm = pwm.unwrap_or_default();
			}

			for (sensor, message_bat_sensor) in sensor_group.bats.iter().zip(&mut *message_sensor_group.bats) {
				let capacity = hwmon::parse_bat_capacity_sensor(&sensor.capacity_path, &mut buf)?;
				message_bat_sensor.capacity = capacity.unwrap_or_default();
				let charging = hwmon::parse_bat_status_sensor(&sensor.status_path, &mut buf)?;
				message_bat_sensor.charging = charging.unwrap_or_default();
			}
		}

		for ((&network, previous_network), message_network) in networks.iter().zip(&mut *previous_networks).zip(&mut *message_networks) {
			let (rx, tx) =
				if previous_network.rx == 0 && previous_network.tx == 0 {
					(0., 0.)
				}
				else if let Some(duration) = network.now.checked_duration_since(previous_network.now) {
					#[allow(clippy::cast_precision_loss)]
					let rx = (network.rx - previous_network.rx) as f64 / (duration.as_millis() as f64 / 1000.);
					#[allow(clippy::cast_precision_loss)]
					let tx = (network.tx - previous_network.tx) as f64 / (duration.as_millis() as f64 / 1000.);
					(rx, tx)
				}
				else {
					(0., 0.)
				};

			*previous_network = network;

			message_network.rx = rx;
			message_network.tx = tx;
		}

		let body = sensord_common::SensorsMessage {
			num_cpus,
			cpus: std::borrow::Cow::Borrowed(&message_cpus),
			cpu_average_usage,
			sensors: std::borrow::Cow::Borrowed(&*message_sensor_groups),
			networks: std::borrow::Cow::Borrowed(&message_networks),
		};

		let body = dbus_pure::proto::AsVariant::as_variant(&body);

		let _ = dbus_client.send(
			&mut dbus_pure::proto::MessageHeader {
				r#type: dbus_pure::proto::MessageType::Signal {
					interface: "dev.arnavion.sensord.Daemon".into(),
					member: "Sensors".into(),
					path: dbus_pure::proto::ObjectPath("/dev/arnavion/sensord/Daemon".into()),
				},
				flags: dbus_pure::proto::message_flags::NO_REPLY_EXPECTED,
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
		if let Some(sleep_duration) = (iteration_start + interval).checked_duration_since(iteration_end) {
			std::thread::sleep(sleep_duration);
		}
	}

	Ok(())
}

struct Error {
	inner: Box<dyn std::error::Error>,
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

impl<E> From<E> for Error where E: Into<Box<dyn std::error::Error>> {
	fn from(err: E) -> Self {
		Error {
			inner: err.into(),
		}
	}
}

#[dbus_pure_macros::interface("org.freedesktop.DBus")]
trait OrgFreeDesktopDbusInterface {
	#[name = "RequestName"]
	fn request_name(name: &str, flags: u32) -> u32;
}

#[dbus_pure_macros::object(OrgFreeDesktopDbusInterface)]
struct OrgFreeDesktopDbusObject;
