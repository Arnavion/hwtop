#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]

#[derive(Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct SensorsMessage<'a> {
	pub num_cpus: u32,
	pub cpus: std::borrow::Cow<'a, [Cpu]>,
	pub cpu_average_usage: f64,
	pub sensors: std::borrow::Cow<'a, [SensorGroup<'a>]>,
	pub networks: std::borrow::Cow<'a, [Network<'a>]>,
}

#[derive(Clone, Copy, Debug, Default, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct Cpu {
	pub usage: f64,
	pub frequency: f64,
}

#[derive(Clone, Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct SensorGroup<'a> {
	pub name: std::borrow::Cow<'a, str>,
	pub temps: Vec<TempSensor<'a>>,
	pub fans: Vec<FanSensor<'a>>,
	pub bats: Vec<BatSensor<'a>>,
}

#[derive(Clone, Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct TempSensor<'a> {
	pub name: std::borrow::Cow<'a, str>,
	pub value: f64,
}

#[derive(Clone, Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct FanSensor<'a> {
	pub name: std::borrow::Cow<'a, str>,
	pub fan: u16,
	pub pwm: u8,
}

#[derive(Clone, Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct BatSensor<'a> {
	pub name: std::borrow::Cow<'a, str>,
	pub capacity: u8,
	pub charging: bool,
}

#[derive(Clone, Debug, dbus_pure_macros::ToVariant, serde::Deserialize)]
pub struct Network<'a> {
	pub name: std::borrow::Cow<'a, str>,
	pub rx: f64,
	pub tx: f64,
}
