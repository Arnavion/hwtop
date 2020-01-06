#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]

#[derive(Debug, serde_derive::Deserialize)]
pub struct SensorsMessage {
	pub num_cpus: u32,
	pub cpus: Vec<Cpu>,
	pub cpu_average_usage: f64,
	pub sensors: Vec<SensorGroup>,
	pub networks: Vec<Network>,
}

#[derive(Clone, Copy, Debug, serde_derive::Deserialize)]
pub struct Cpu {
	pub usage: f64,
	pub frequency: f64,
}

#[derive(Debug, serde_derive::Deserialize)]
pub struct SensorGroup {
	pub name: String,
	pub temps: Vec<TempSensor>,
	pub fans: Vec<FanSensor>,
}

#[derive(Debug, serde_derive::Deserialize)]
pub struct TempSensor {
	pub name: String,
	pub value: f64,
}

#[derive(Debug, serde_derive::Deserialize)]
pub struct FanSensor {
	pub name: String,
	pub fan: u16,
	pub pwm: u8,
}

#[derive(Debug, serde_derive::Deserialize)]
pub struct Network {
	pub name: String,
	pub rx: f64,
	pub tx: f64,
}
