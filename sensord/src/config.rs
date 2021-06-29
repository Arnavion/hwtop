#[derive(Debug)]
pub(crate) struct Config {
	pub(crate) interval: std::time::Duration,
	pub(crate) cpus: Cpus,
	pub(crate) sensors: Vec<SensorGroup>,
	pub(crate) networks: Vec<Network>,
}

#[derive(Debug, Default, serde_derive::Deserialize)]
pub(crate) struct Cpus {
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
	pub(crate) path: Option<std::path::PathBuf>,
	pub(crate) offset: f64,
	pub(crate) name: Option<String>,
}

#[derive(Debug)]
pub(crate) struct FanSensor {
	pub(crate) fan_path: Option<std::path::PathBuf>,
	pub(crate) pwm_path: Option<std::path::PathBuf>,
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
			#[serde(default)]
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
			offset: Option<f64>,
			name: Option<String>,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		#[serde(untagged)]
		enum InnerTempSensorSpec {
			Hwmon { hwmon: String, #[serde(flatten)] num_or_label: HwmonNumOrLabel },
			Thermal { thermal_zone: u8 },
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct InnerFanSensor {
			hwmon: String,
			#[serde(flatten)] num_or_label: HwmonNumOrLabel,
			name: Option<String>,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct HwmonNum {
			num: u8,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		struct HwmonLabel {
			label: String,
		}

		#[derive(Debug, serde_derive::Deserialize)]
		#[serde(untagged)]
		enum HwmonNumOrLabel {
			Num(HwmonNum),
			Label(HwmonLabel),
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
						if let Ok(mut name) = std::fs::read_to_string(name_file) {
							if name.pop() == Some('\n') && name == dev_name {
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
						InnerTempSensorSpec::Hwmon { hwmon: sensor_hwmon, num_or_label } => {
							let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| format!("hwmon {:?} is not defined", sensor_hwmon))?;

							let num = match num_or_label {
								HwmonNumOrLabel::Num(HwmonNum { num }) => Some(num),

								HwmonNumOrLabel::Label(HwmonLabel { label: expected_label }) => {
									let mut num = None;

									for entry in std::fs::read_dir(hwmon)? {
										let entry = entry?.path();

										let entry_file_name = match entry.file_name().and_then(std::ffi::OsStr::to_str) {
											Some(entry_file_name) => entry_file_name,
											None => continue,
										};
										if !entry_file_name.starts_with("temp") || !entry_file_name.ends_with("_label") {
											continue;
										}

										if let Ok(mut actual_label) = std::fs::read_to_string(&entry) {
											if actual_label.pop() == Some('\n') && actual_label == expected_label {
												let actual_num = &entry_file_name[("temp".len())..(entry_file_name.len() - "_label".len())];
												if let Ok(actual_num) = actual_num.parse() {
													num = Some(actual_num);
													break;
												}
											}
										}
									}

									num
								},
							};

							let name = name.or_else(|| num.and_then(|num| {
								let label_path = hwmon.join(format!("temp{}_label", num));
								if let Ok(mut label) = std::fs::read_to_string(label_path) {
									if label.pop() == Some('\n') {
										Some(label)
									}
									else {
										None
									}
								}
								else {
									None
								}
							}));

							Ok(TempSensor {
								path: num.map(|num| hwmon.join(format!("temp{}_input", num))),
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

							thermal.push("temp");

							Ok(TempSensor {
								path: Some(thermal),
								offset: offset.unwrap_or_default(),
								name,
							})
						},
					})
					.collect();
				let temps = temps?;

				let fans: Result<_, super::Error> =
					fans.into_iter()
					.map(|InnerFanSensor { hwmon: sensor_hwmon, num_or_label, name }| {
						let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| format!("hwmon {:?} is not defined", sensor_hwmon))?;

						let num = match num_or_label {
							HwmonNumOrLabel::Num(HwmonNum { num }) => Some(num),

							HwmonNumOrLabel::Label(HwmonLabel { label: expected_label }) => {
								let mut num = None;

								for entry in std::fs::read_dir(hwmon)? {
									let entry = entry?.path();

									let entry_file_name = match entry.file_name().and_then(std::ffi::OsStr::to_str) {
										Some(entry_file_name) => entry_file_name,
										None => continue,
									};
									if !entry_file_name.starts_with("fan") || !entry_file_name.ends_with("_label") {
										continue;
									}

									if let Ok(mut actual_label) = std::fs::read_to_string(&entry) {
										if actual_label.pop() == Some('\n') && actual_label == expected_label {
											let actual_num = &entry_file_name[("fan".len())..(entry_file_name.len() - "_label".len())];
											if let Ok(actual_num) = actual_num.parse() {
												num = Some(actual_num);
												break;
											}
										}
									}
								}

								num
							},
						};

						let name = name.or_else(|| num.and_then(|num| {
							let label_path = hwmon.join(format!("fan{}_label", num));
							if let Ok(label) = std::fs::read_to_string(label_path) {
								Some(label.trim().to_owned())
							}
							else {
								None
							}
						}));

						Ok(FanSensor {
							fan_path: num.map(|num| hwmon.join(format!("fan{}_input", num))),
							pwm_path: num.map(|num| hwmon.join(format!("pwm{}", num))),
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

		let networks =
			networks.into_iter()
			.map(|network| {
				let mut dir: std::path::PathBuf = "/sys/class/net".into();
				dir.push(&network);
				let mut dir = std::fs::canonicalize(&dir).unwrap_or(dir);
				dir.push("statistics");
				let rx_path = dir.join("rx_bytes");
				let tx_path = dir.join("tx_bytes");
				Network {
					name: network,
					rx_path,
					tx_path,
				}
			})
			.collect();

		Ok(Config {
			interval,
			cpus,
			sensors,
			networks,
		})
	}
}
