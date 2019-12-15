#[derive(Debug)]
pub(crate) struct Config {
	pub(crate) interval: std::time::Duration,
	pub(crate) cpus: Cpus,
	pub(crate) sensors: Vec<SensorGroup>,
	pub(crate) networks: Vec<Network>,
}

#[derive(Debug, serde_derive::Deserialize)]
pub(crate) struct Cpus {
	pub(crate) cols: usize,
	#[serde(default)]
	pub(crate) use_sysfs: bool,
}

#[derive(Debug)]
pub(crate) struct SensorGroup {
	pub(crate) name: String,
	pub(crate) temps: Vec<TempSensor>,
	pub(crate) fans: Vec<FanSensor>,
}

#[derive(Debug)]
pub(crate) struct TempSensor {
	pub(crate) path: std::path::PathBuf,
	pub(crate) offset: f32,
	pub(crate) name: Option<String>,
}

#[derive(Debug)]
pub(crate) struct FanSensor {
	pub(crate) fan_path: std::path::PathBuf,
	pub(crate) pwm_path: std::path::PathBuf,
	pub(crate) name: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Network {
	pub(crate) name: String,
	pub(crate) rx_path: std::path::PathBuf,
	pub(crate) tx_path: std::path::PathBuf,
}

impl<'de> serde::Deserialize<'de> for Config {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
		#[derive(Debug, serde_derive::Deserialize)]
		struct InnerConfig {
			interval: Option<f32>,
			cpus: Cpus,
			#[serde(default)]
			hwmon: std::collections::BTreeMap<String, Hwmon>,
			#[serde(default)]
			sensors: Vec<InnerSensorGroup>,
			#[serde(default)]
			networks: Vec<String>,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		enum Hwmon {
			#[serde(rename = "dev_name")]
			Name(String),
			#[serde(rename = "dev_path")]
			Path(std::path::PathBuf),
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct InnerSensorGroup {
			name: String,
			#[serde(default)]
			temps: Vec<InnerTempSensor>,
			#[serde(default)]
			fans: Vec<InnerFanSensor>,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct InnerTempSensor {
			#[serde(flatten)]
			spec: InnerTempSensorSpec,
			offset: Option<f32>,
			name: Option<String>,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		#[serde(untagged)]
		enum InnerTempSensorSpec {
			Hwmon { hwmon: String, num: u8 },
			Thermal { thermal_zone: u8 },
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct InnerFanSensor {
			hwmon: String,
			num: u8,
			name: Option<String>,
		}

		let InnerConfig { interval, cpus, hwmon, sensors, networks } = serde::Deserialize::deserialize(deserializer)?;

		let interval = interval.unwrap_or(1.);
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		let interval = std::time::Duration::from_millis((interval * 1000.) as u64);

		let mut already_discovered_hwmon: std::collections::BTreeSet<_> = Default::default();
		let hwmon: Result<std::collections::BTreeMap<_, _>, super::Error> =
			hwmon.into_iter()
			.map(|(hwmon_name, hwmon)| match hwmon {
				Hwmon::Name(dev_name) => {
					for dir in std::fs::read_dir("/sys/class/hwmon")? {
						let dir = dir?.path();
						let name_file = dir.join("name");
						if let Ok(name) = std::fs::read_to_string(name_file) {
							if name.trim() == dev_name {
								let dir = std::fs::canonicalize(dir)?;
								if already_discovered_hwmon.insert(dir.clone()) {
									return Ok((hwmon_name, dir));
								}
							}
						}
					}

					Err(format!("could not find hwmon named {}", dev_name).into())
				},

				Hwmon::Path(mut dev_path) => {
					dev_path.push("hwmon");
					if let Some(dir) = std::fs::read_dir(&dev_path)?.next() {
						let dir = std::fs::canonicalize(dir?.path())?;
						if already_discovered_hwmon.insert(dir.clone()) {
							Ok((hwmon_name, dir))
						}
						else {
							Err(format!("hwmon path {} was already found before", dir.display()).into())
						}
					}
					else {
						Err(format!("could not find hwmon path under {}", dev_path.display()).into())
					}
				},
			})
			.collect();
		let hwmon = hwmon.map_err(serde::de::Error::custom)?;

		let sensors: Result<_, super::Error> =
			sensors.into_iter()
			.map(|InnerSensorGroup { name, temps, fans }| {
				let temps: Result<_, super::Error> =
					temps.into_iter()
					.map(|InnerTempSensor { spec, offset, name }| match spec {
						InnerTempSensorSpec::Hwmon { hwmon: sensor_hwmon, num } => {
							let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| format!("hwmon {:?} is not defined", sensor_hwmon))?;

							let name = name.or_else(|| {
								let label_path = hwmon.join(format!("temp{}_label", num));
								if let Ok(label) = std::fs::read_to_string(label_path) {
									Some(label.trim().to_owned())
								}
								else {
									None
								}
							});

							Ok(TempSensor {
								path: hwmon.join(format!("temp{}_input", num)),
								offset: offset.unwrap_or_default(),
								name,
							})
						},

						InnerTempSensorSpec::Thermal { thermal_zone } => {
							let mut thermal = std::path::Path::new("/sys/class/thermal").to_owned();
							thermal.push(format!("thermal_zone{}", thermal_zone));

							let name = name.or_else(|| {
								let label_path = thermal.join("type");
								if let Ok(label) = std::fs::read_to_string(label_path) {
									Some(label.trim().to_owned())
								}
								else {
									None
								}
							});

							Ok(TempSensor {
								path: thermal.join("temp"),
								offset: offset.unwrap_or_default(),
								name,
							})
						},
					})
					.collect();
				let temps = temps?;

				let fans: Result<_, super::Error> =
					fans.into_iter()
					.map(|InnerFanSensor { hwmon: sensor_hwmon, num, name }| {
						let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| format!("hwmon {:?} is not defined", sensor_hwmon))?;

						let name = name.or_else(|| {
							let label_path = hwmon.join(format!("fan{}_label", num));
							if let Ok(label) = std::fs::read_to_string(label_path) {
								Some(label.trim().to_owned())
							}
							else {
								None
							}
						});

						Ok(FanSensor {
							fan_path: hwmon.join(format!("fan{}_input", num)),
							pwm_path: hwmon.join(format!("pwm{}", num)),
							name,
						})
					})
					.collect();
				let fans = fans?;

				Ok(SensorGroup {
					name,
					temps,
					fans,
				})
			})
			.collect();
		let sensors = sensors.map_err(serde::de::Error::custom)?;

		let networks: Result<_, super::Error> =
			networks.into_iter()
			.map(|network| {
				let mut dir: std::path::PathBuf = "/sys/class/net".into();
				dir.push(&network);
				let mut dir = std::fs::canonicalize(dir)?;
				dir.push("statistics");
				let rx_path = dir.join("rx_bytes");
				let tx_path = dir.join("tx_bytes");
				Ok(Network {
					name: network,
					rx_path,
					tx_path,
				})
			})
			.collect();
		let networks = networks.map_err(serde::de::Error::custom)?;

		Ok(Config {
			interval,
			cpus,
			sensors,
			networks,
		})
	}
}
