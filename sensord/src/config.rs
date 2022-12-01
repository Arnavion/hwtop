#[derive(Debug)]
pub(crate) struct Config {
	pub(crate) interval: std::time::Duration,
	pub(crate) cpus: Cpus,
	pub(crate) sensors: Vec<SensorGroup>,
	pub(crate) networks: Vec<Network>,
}

#[derive(Debug, Default, Eq, PartialEq, serde::Deserialize)]
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
		let InnerConfig { interval, cpus, hwmon, power_supply, sensors, networks } = serde::Deserialize::deserialize(deserializer)?;

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

		let power_supply: Result<std::collections::BTreeMap<_, _>, crate::Error> =
			power_supply.into_iter()
			.map(|(power_supply_name, dev_name)| {
				let dir = std::path::Path::new("/sys/class/power_supply").join(dev_name);
				let dir = crate::std2::fs::canonicalize(&dir)?;
				Ok((power_supply_name, dir))
			})
			.collect();
		let power_supply = power_supply.map_err(serde::de::Error::custom)?;

		let sensors: Result<_, crate::Error> =
			sensors.into_iter()
			.map(|InnerSensorGroup { name, temps, fans, bats }| {
				let temps: Result<_, crate::Error> =
					temps.into_iter()
					.map(|InnerTempSensor { spec, offset, name }| match spec {
						InnerTempSensorSpec::Hwmon { hwmon: sensor_hwmon, num_or_label } => {
							let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| crate::Error::Other(format!("hwmon {sensor_hwmon:?} is not defined").into()))?;

							let num = match num_or_label {
								HwmonNumOrLabel::Num(num) => Some(num),

								HwmonNumOrLabel::Label(expected_label) => {
									let mut num = None;

									for entry in crate::std2::fs::read_dir(hwmon)? {
										let entry = entry?.path();

										let Some(entry_file_name) = entry.file_name().and_then(std::ffi::OsStr::to_str) else { continue; };
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
							HwmonNumOrLabel::Num(num) => Some(num),

							HwmonNumOrLabel::Label(expected_label) => {
								let mut num = None;

								for entry in crate::std2::fs::read_dir(hwmon)? {
									let entry = entry?.path();

									let Some(entry_file_name) = entry.file_name().and_then(std::ffi::OsStr::to_str) else { continue; };
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
					.map(|bat| match bat {
						InnerBatSensor::Hwmon(sensor_hwmon) => {
							let hwmon = hwmon.get(&sensor_hwmon).ok_or_else(|| crate::Error::Other(format!("hwmon {sensor_hwmon:?} is not defined").into()))?;

							let mut device_path = hwmon.clone();
							device_path.push("device");

							let capacity_path = device_path.join("capacity");

							let status_path = {
								let mut status_path = device_path;
								status_path.push("status");
								status_path
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
						},

						InnerBatSensor::PowerSupply(sensor_power_supply) => {
							let power_supply =
								power_supply.get(&sensor_power_supply)
								.ok_or_else(|| crate::Error::Other(format!("power_supply {sensor_power_supply:?} is not defined").into()))?;

							let capacity_path = power_supply.join("capacity");

							let status_path = power_supply.join("status");

							let name = {
								let mut name_path = power_supply.join("device");
								name_path.push("name");
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
						},
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

#[derive(Debug, PartialEq, serde::Deserialize)]
struct InnerConfig {
	interval: Option<f32>,
	#[serde(default)]
	cpus: Cpus,
	#[serde(default)]
	hwmon: std::collections::BTreeMap<String, Hwmon>,
	#[serde(default)]
	power_supply: std::collections::BTreeMap<String, String>,
	#[serde(default, rename = "sensor")]
	sensors: Vec<InnerSensorGroup>,
	#[serde(default)]
	networks: Vec<String>,
}

#[derive(Debug, Eq, PartialEq, serde::Deserialize)]
enum Hwmon {
	#[serde(rename = "dev_name")]
	Name(String),
	#[serde(rename = "dev_path")]
	Path(std::path::PathBuf),
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct InnerSensorGroup {
	name: String,
	#[serde(default)]
	temps: Vec<InnerTempSensor>,
	#[serde(default)]
	fans: Vec<InnerFanSensor>,
	#[serde(default)]
	bats: Vec<InnerBatSensor>,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
struct InnerTempSensor {
	#[serde(flatten)]
	spec: InnerTempSensorSpec,
	offset: Option<f64>,
	name: Option<String>,
}

#[derive(Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(untagged)]
enum InnerTempSensorSpec {
	Hwmon { hwmon: String, #[serde(flatten)] num_or_label: HwmonNumOrLabel },
	Thermal { thermal_zone: u8 },
}

#[derive(Debug, Eq, PartialEq, serde::Deserialize)]
struct InnerFanSensor {
	hwmon: String,
	#[serde(flatten)] num_or_label: HwmonNumOrLabel,
	name: Option<String>,
}

#[derive(Debug, Eq, PartialEq, serde::Deserialize)]
enum InnerBatSensor {
	#[serde(rename = "hwmon")]
	Hwmon(String),
	#[serde(rename = "power_supply")]
	PowerSupply(String),
}

#[derive(Debug, Eq, PartialEq, serde::Deserialize)]
enum HwmonNumOrLabel {
	#[serde(rename = "num")]
	Num(u8),
	#[serde(rename = "label")]
	Label(String),
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn microsoft_surfacert() {
		test_inner("microsoft-surfacert.toml", &InnerConfig {
			interval: None,
			cpus: Cpus {
				use_sysfs: false,
			},
			hwmon: [
				("soc".to_owned(), Hwmon::Name("nct1008".to_owned())),
			].into(),
			power_supply: [
				("bat".to_owned(), "surface-rt-battery".to_owned()),
			].into(),
			sensors: vec![
				InnerSensorGroup {
					name: "SoC".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "soc".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "soc".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(2),
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "Bat".to_owned(),
					temps: vec![],
					fans: vec![],
					bats: vec![
						InnerBatSensor::PowerSupply("bat".to_owned()),
					],
				},
			],
			networks: vec![
				"mlan0".to_owned(),
			],
		});
	}

	#[test]
	fn pinephone() {
		test_inner("pinephone.toml", &InnerConfig {
			interval: None,
			cpus: Cpus {
				use_sysfs: true,
			},
			hwmon: [
				("cpu".to_owned(), Hwmon::Name("cpu0_thermal".to_owned())),
				("gpu0".to_owned(), Hwmon::Name("gpu0_thermal".to_owned())),
				("gpu1".to_owned(), Hwmon::Name("gpu1_thermal".to_owned())),
				("bat".to_owned(), Hwmon::Name("axp20x_battery".to_owned())),
			].into(),
			power_supply: Default::default(),
			sensors: vec![
				InnerSensorGroup {
					name: "CPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "GPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu0".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu1".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "Bat".to_owned(),
					temps: vec![],
					fans: vec![],
					bats: vec![
						InnerBatSensor::Hwmon("bat".to_owned()),
					],
				},
			],
			networks: vec![
				"eth0".to_owned(),
				"wwan0".to_owned(),
			],
		});
	}

	#[test]
	fn raspberry_pi() {
		test_inner("raspberry-pi.toml", &InnerConfig {
			interval: None,
			cpus: Cpus {
				use_sysfs: true,
			},
			hwmon: [
				("cpu".to_owned(), Hwmon::Name("cpu_thermal".to_owned())),
			].into(),
			power_supply: Default::default(),
			sensors: vec![
				InnerSensorGroup {
					name: "CPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Thermal {
								thermal_zone: 0,
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![],
					bats: vec![],
				},
			],
			networks: vec![
				"eth0".to_owned(),
			],
		});
	}

	#[test]
	fn t61() {
		test_inner("t61.toml", &InnerConfig {
			interval: None,
			cpus: Cpus {
				use_sysfs: false,
			},
			hwmon: [
				("acpi".to_owned(), Hwmon::Name("acpitz".to_owned())),
				("cpu".to_owned(), Hwmon::Name("coretemp".to_owned())),
				("gpu".to_owned(), Hwmon::Name("nouveau".to_owned())),
				("mobo".to_owned(), Hwmon::Name("thinkpad".to_owned())),
			].into(),
			power_supply: Default::default(),
			sensors: vec![
				InnerSensorGroup {
					name: "CPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Core 0".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Core 1".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: Some("mobo".to_owned()),
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "GPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(1),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(4),
							},
							offset: None,
							name: Some("mobo".to_owned()),
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "Mobo".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(2),
							},
							offset: None,
							name: Some("aps".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(3),
							},
							offset: None,
							name: Some("crd".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(5),
							},
							offset: None,
							name: Some("no5".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(9),
							},
							offset: None,
							name: Some("bus".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(10),
							},
							offset: None,
							name: Some("pci".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(11),
							},
							offset: None,
							name: Some("pwr".to_owned()),
						},
					],
					fans: vec![
						InnerFanSensor {
							hwmon: "mobo".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(1),
							name: None,
						},
					],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "Mobo".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(6),
							},
							offset: None,
							name: Some("x7d".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(7),
							},
							offset: None,
							name: Some("bat".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(8),
							},
							offset: None,
							name: Some("x7f".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Num(12),
							},
							offset: None,
							name: Some("xc3".to_owned()),
						},
					],
					fans: vec![],
					bats: vec![],
				},
			],
			networks: vec![
				"enp0s25".to_owned(),
			],
		});
	}

	#[test]
	fn threadripper2() {
		test_inner("threadripper2.toml", &InnerConfig {
			interval: None,
			cpus: Cpus {
				use_sysfs: false,
			},
			hwmon: [
				("cpu1".to_owned(), Hwmon::Name("k10temp".to_owned())),
				("cpu2".to_owned(), Hwmon::Name("k10temp".to_owned())),
				("gpu".to_owned(), Hwmon::Name("amdgpu".to_owned())),
				("mobo".to_owned(), Hwmon::Name("nct6779".to_owned())),
			].into(),
			power_supply: Default::default(),
			sensors: vec![
				InnerSensorGroup {
					name: "CPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu1".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Tdie".to_owned()),
							},
							offset: None,
							name: Some("CPU 1".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu2".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Tdie".to_owned()),
							},
							offset: None,
							name: Some("CPU 2".to_owned()),
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("SMBUSMASTER 0".to_owned()),
							},
							offset: Some(-27.0),
							name: None,
						},
					],
					fans: vec![
						InnerFanSensor {
							hwmon: "mobo".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(2),
							name: Some("Fan 1".to_owned()),
						},
					],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "CPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu1".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Tccd1".to_owned()),
							},
							offset: Some(-27.0),
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "cpu2".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("Tccd1".to_owned()),
							},
							offset: Some(-27.0),
							name: None,
						},
					],
					fans: vec![],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "GPU".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("edge".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("junction".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "gpu".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("mem".to_owned()),
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![
						InnerFanSensor {
							hwmon: "gpu".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(1),
							name: None,
						},
					],
					bats: vec![],
				},
				InnerSensorGroup {
					name: "Mobo".to_owned(),
					temps: vec![
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("SYSTIN".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("AUXTIN1".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("AUXTIN2".to_owned()),
							},
							offset: None,
							name: None,
						},
						InnerTempSensor {
							spec: InnerTempSensorSpec::Hwmon {
								hwmon: "mobo".to_owned(),
								num_or_label: HwmonNumOrLabel::Label("AUXTIN3".to_owned()),
							},
							offset: None,
							name: None,
						},
					],
					fans: vec![
						InnerFanSensor {
							hwmon: "mobo".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(1),
							name: Some("Front".to_owned()),
						},
						InnerFanSensor {
							hwmon: "mobo".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(4),
							name: Some("Side".to_owned()),
						},
						InnerFanSensor {
							hwmon: "mobo".to_owned(),
							num_or_label: HwmonNumOrLabel::Num(5),
							name: Some("Rear".to_owned()),
						},
					],
					bats: vec![],
				},
			],
			networks: vec![
				"enp4s0".to_owned(),
			],
		});
	}

	fn test_inner(filename: &str, expected: &InnerConfig) {
		let mut path: std::path::PathBuf = std::env::var_os("CARGO_MANIFEST_DIR").unwrap().into();
		path.push("config-examples");
		path.push(filename);
		let actual = std::fs::read(path).unwrap();
		let actual = toml::from_slice(&actual).unwrap();
		assert_eq!(*expected, actual);
	}
}
