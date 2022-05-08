#[derive(Debug)]
pub(crate) struct Config {
	pub(crate) interval: std::time::Duration,
	pub(crate) cpus: Cpus,
	pub(crate) sensors: Vec<SensorGroup>,
	pub(crate) networks: Vec<Network>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct Cpus {
	#[serde(default)]
	pub(crate) use_sysfs: bool,
}

#[derive(Debug)]
pub(crate) struct SensorGroup {
	pub(crate) name: String,
	pub(crate) temps: Vec<TempSensor>,
	pub(crate) fans: Vec<FanSensor>,
	pub(crate) bats: Vec<BatSensor>,
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
pub(crate) struct BatSensor {
	pub(crate) capacity_path: std::path::PathBuf,
	pub(crate) status_path: std::path::PathBuf,
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
		#[derive(Debug, serde::Deserialize)]
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

		#[derive(Debug, serde::Deserialize)]
		enum Hwmon {
			#[serde(rename = "dev_name")]
			Name(String),
			#[serde(rename = "dev_path")]
			Path(std::path::PathBuf),
		}

		#[derive(Debug, serde::Deserialize)]
		struct InnerSensorGroup {
			name: String,
			#[serde(default)]
			temps: Vec<InnerTempSensor>,
			#[serde(default)]
			fans: Vec<InnerFanSensor>,
			#[serde(default)]
			bats: Vec<InnerBatSensor>,
		}

		#[derive(Debug, serde::Deserialize)]
		struct InnerTempSensor {
			#[serde(flatten)]
			spec: InnerTempSensorSpec,
			offset: Option<f64>,
			name: Option<String>,
		}

		#[derive(Debug, serde::Deserialize)]
		#[serde(untagged)]
		enum InnerTempSensorSpec {
			Hwmon { hwmon: String, #[serde(flatten)] num_or_label: HwmonNumOrLabel },
			Thermal { thermal_zone: u8 },
		}

		#[derive(Debug, serde::Deserialize)]
		struct InnerFanSensor {
			hwmon: String,
			#[serde(flatten)] num_or_label: HwmonNumOrLabel,
			name: Option<String>,
		}

		#[derive(Debug, serde::Deserialize)]
		struct InnerBatSensor {
			hwmon: String,
		}

		#[derive(Debug, serde::Deserialize)]
		struct HwmonNum {
			num: u8,
		}

		#[derive(Debug, serde::Deserialize)]
		struct HwmonLabel {
			label: String,
		}

		#[derive(Debug, serde::Deserialize)]
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
		let hwmon: Result<std::collections::BTreeMap<_, _>, crate::Error> =
			hwmon.into_iter()
			.map(|(hwmon_name, hwmon)| match hwmon {
				Hwmon::Name(dev_name) => {
					for dir in crate::std2::fs::read_dir("/sys/class/hwmon".as_ref())? {
						let dir = dir?.path();
						let name_file = dir.join("name");
						if let Ok(mut name) = std::fs::read_to_string(name_file) {
							if name.pop() == Some('\n') && name == dev_name {
								let dir = crate::std2::fs::canonicalize(&dir)?;
								if already_discovered_hwmon.insert(dir.clone()) {
									return Ok((hwmon_name, dir));
								}
							}
						}
					}

					Err(crate::Error::Other(format!("could not find hwmon named {dev_name}").into()))
				},

				Hwmon::Path(mut dev_path) => {
					dev_path.push("hwmon");
					if let Some(dir) = crate::std2::fs::read_dir(&dev_path)?.next() {
						let dir = crate::std2::fs::canonicalize(&dir?.path())?;
						if already_discovered_hwmon.insert(dir.clone()) {
							Ok((hwmon_name, dir))
						}
						else {
							Err(crate::Error::Other(format!("hwmon path {} was already found before", dir.display()).into()))
						}
					}
					else {
						Err(crate::Error::Other(format!("could not find hwmon path under {}", dev_path.display()).into()))
					}
				},
			})
			.collect();
		let hwmon = hwmon.map_err(serde::de::Error::custom)?;

		let sensors: Result<_, crate::Error> =
			sensors.into_iter()
			.map(|InnerSensorGroup { name, temps, fans, bats }| {
				let temps: Result<_, crate::Error> =
					temps.into_iter()
					.map(|InnerTempSensor { spec, offset, name }| match spec {
						InnerTempSensorSpec::Hwmon { hwmon: sensor_hwmon, num_or_label } => {
							let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| crate::Error::Other(format!("hwmon {sensor_hwmon:?} is not defined").into()))?;

							let num = match num_or_label {
								HwmonNumOrLabel::Num(HwmonNum { num }) => Some(num),

								HwmonNumOrLabel::Label(HwmonLabel { label: expected_label }) => {
									let mut num = None;

									for entry in crate::std2::fs::read_dir(hwmon)? {
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
								let label_path = hwmon.join(format!("temp{num}_label"));
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
								path: num.map(|num| hwmon.join(format!("temp{num}_input"))),
								offset: offset.unwrap_or_default(),
								name,
							})
						},

						InnerTempSensorSpec::Thermal { thermal_zone } => {
							let mut thermal = std::path::Path::new("/sys/class/thermal").to_owned();
							thermal.push(format!("thermal_zone{thermal_zone}"));

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

				let fans: Result<_, crate::Error> =
					fans.into_iter()
					.map(|InnerFanSensor { hwmon: sensor_hwmon, num_or_label, name }| {
						let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| crate::Error::Other(format!("hwmon {sensor_hwmon:?} is not defined").into()))?;

						let num = match num_or_label {
							HwmonNumOrLabel::Num(HwmonNum { num }) => Some(num),

							HwmonNumOrLabel::Label(HwmonLabel { label: expected_label }) => {
								let mut num = None;

								for entry in crate::std2::fs::read_dir(hwmon)? {
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
							let label_path = hwmon.join(format!("fan{num}_label"));
							if let Ok(label) = std::fs::read_to_string(label_path) {
								Some(label.trim().to_owned())
							}
							else {
								None
							}
						}));

						Ok(FanSensor {
							fan_path: num.map(|num| hwmon.join(format!("fan{num}_input"))),
							pwm_path: num.map(|num| hwmon.join(format!("pwm{num}"))),
							name,
						})
					})
					.collect();
				let fans = fans?;

				let bats: Result<_, crate::Error> =
					bats.into_iter()
					.map(|InnerBatSensor { hwmon: sensor_hwmon }| {
						let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| crate::Error::Other(format!("hwmon {sensor_hwmon:?} is not defined").into()))?;

						let mut device_path = hwmon.clone();
						device_path.push("device");

						let capacity_path = {
							let mut capacity_path = device_path.clone();
							capacity_path.push("capacity");
							capacity_path
						};

						let status_path = {
							let mut capacity_path = device_path;
							capacity_path.push("status");
							capacity_path
						};

						let name = {
							let name_path = hwmon.join("name");
							if let Ok(name) = std::fs::read_to_string(name_path) {
								Some(name.trim().to_owned())
							}
							else {
								None
							}
						};

						Ok(BatSensor {
							capacity_path,
							status_path,
							name,
						})
					})
					.collect();
				let bats = bats?;

				Ok(SensorGroup {
					name,
					temps,
					fans,
					bats,
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
