use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    str::from_utf8,
    time::Duration,
};

use anyhow::{anyhow, Result};
use csv::ReaderBuilder;
use itertools::{iproduct, izip, Itertools};

const SP_PATTERN: &str = "Sample Period (ms)";
const PROBE_PATTERN: &str = "Capacitive Probe (m)";
const CURRENT_PATTERN: &str = "Current (A)";
const VOLTAGE_PATTERN: &str = "Voltage Monitor (V)";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavegenSettings {
    pub pkpk: f64,
    pub period: Duration,
    pub symmetry_p: f64,
    pub offset: f64,
}
impl WavegenSettings {
    pub fn set_ramp_time(&mut self, ramp_time: Duration, rest_time: Duration) {
        self.period = (ramp_time + rest_time) * 2;
        self.symmetry_p = ramp_time.as_secs_f64() / (self.period.as_secs_f64() / 2.) * 100.;
    }
}
impl Default for WavegenSettings {
    fn default() -> Self {
        Self {
            pkpk: Default::default(),
            period: Default::default(),
            symmetry_p: Default::default(),
            offset: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct Aquisition {
    pub probe: Vec<f64>,
    pub current: Vec<f64>,
    pub voltage: Vec<f64>,
    pub wavegen_settings: WavegenSettings,
    pub sample_period_ms: f64,
}
impl Aquisition {
    pub fn combine(mut self, other: Aquisition) -> Aquisition {
        assert_eq!(self.wavegen_settings, other.wavegen_settings);
        assert_eq!(self.sample_period_ms, other.sample_period_ms);
        let inds = |vec: &Vec<f64>, v: &f64| vec.iter().positions(|f| f == v).collect_vec();
        let intersect = |v1: &Vec<usize>, v2: &Vec<usize>| {
            iproduct!(v1, v2)
                .filter(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect_vec()
        };
        let last_probe = self.probe.last().unwrap();
        let last_current = self.current.last().unwrap();
        let last_voltage = self.voltage.last().unwrap();
        let mut ind_sets = vec![];
        ind_sets.push(inds(&other.probe, last_probe));
        ind_sets.push(inds(&other.current, last_current));
        ind_sets.push(inds(&other.voltage, last_voltage));
        ind_sets.sort_by_key(|v| v.len());
        let ind = if ind_sets[0].len() == 1 {
            ind_sets[0][0]
        } else if intersect(&ind_sets[0], &ind_sets[1]).len() == 1 {
            intersect(&ind_sets[0], &ind_sets[1])[0]
        } else {
            panic!("{ind_sets:#?}");
        };
        self.probe.extend(other.probe.iter().skip(ind + 1));
        self.current.extend(other.current.iter().skip(ind + 1));
        self.voltage.extend(other.voltage.iter().skip(ind + 1));
        self
    }
    pub fn write_to_writer<W>(&self, mut writer: W) -> std::io::Result<()>
    where
        W: std::io::Write,
    {
        writeln!(writer, "Experiment\tHistory Data\t")?;
        writeln!(
            writer,
            "Date\t{}\t",
            chrono::Local::now().format("%d.%m.%Y %H:%M:%S")
        )?;
        writeln!(
            writer,
            "Sample Period (ms)\t{}\t",
            self.sample_period_ms as usize
        )?;
        writeln!(writer, "pkpk\t{}\t", self.wavegen_settings.pkpk)?;
        writeln!(writer, "offset\t{}\t", self.wavegen_settings.offset)?;
        writeln!(writer, "symmetry_p\t{}\t", self.wavegen_settings.symmetry_p)?;
        writeln!(
            writer,
            "period_s\t{}\t",
            self.wavegen_settings.period.as_secs_f64()
        )?;
        writeln!(writer, "")?;
        writeln!(writer, "[DATA]")?;
        writeln!(
            writer,
            "{PROBE_PATTERN}\t{CURRENT_PATTERN}\t{VOLTAGE_PATTERN}"
        )?;
        let iter = izip!(&self.probe, &self.current, &self.voltage);
        for (d, c, v) in iter {
            writeln!(writer, "{d}\t{c}\t{v}")?;
        }
        Ok(())
    }
    pub fn read_from_file(path: impl AsRef<Path>, settings: WavegenSettings) -> Result<Self> {
        let mut reader = BufReader::new(File::open(path)?);
        let mut buf = vec![];
        let mut sample_period_str = String::new();
        while !from_utf8(&buf[..])?.starts_with("[DATA]") {
            buf.clear();
            reader.read_until(b'\n', &mut buf)?;
            match from_utf8(&buf[..])? {
                str if str.starts_with(SP_PATTERN) => {
                    sample_period_str = str.trim_start_matches(SP_PATTERN).trim().to_string();
                }
                _ => {}
            }
        }
        let mut csv_reader = ReaderBuilder::new().delimiter(b'\t').from_reader(reader);
        let headers = csv_reader
            .headers()?
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let probe_index = headers.iter().position(|h| h == PROBE_PATTERN).unwrap();
        let current_index = headers.iter().position(|h| h == CURRENT_PATTERN).unwrap();
        let voltage_index = headers.iter().position(|h| h == VOLTAGE_PATTERN).unwrap();
        let mut probe = vec![];
        let mut current = vec![];
        let mut voltage = vec![];
        for record in csv_reader.into_records() {
            for (i, value) in record?.iter().map(str::parse::<f64>).enumerate() {
                match i {
                    n if n == probe_index => probe.push(value?),
                    n if n == current_index => current.push(value?),
                    n if n == voltage_index => voltage.push(value?),
                    _ => {}
                }
            }
        }
        Ok(Self {
            wavegen_settings: settings,
            sample_period_ms: sample_period_str.as_str().parse::<f64>()?,
            probe,
            current,
            voltage,
        })
    }
}
