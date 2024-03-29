mod power_automate;

use std::{io::BufWriter, path::PathBuf, time::Duration};

use anyhow::Result;
use power_automate::{AquisitionDriver, WavegenSettings};

#[tokio::main]
async fn main() -> Result<()> {
    let mut aqd = AquisitionDriver::new().await?;

    let folder = PathBuf::from(r#"C:\Users\Brad\Desktop\code\actuator-project\data\pzt-tile\0002"#);
    let num_samples = 2;
    let pkpk = 200.;
    let offset = 200.;

    let hyst_periods = [];
    // let hyst_periods = [0.25, 1., 5., 20.];
    let ramp_times = [0.5];
    // let ramp_times = [0.1, 1., 5.];
    let ramp_rest_time = 120.;

    let mut settings = WavegenSettings::default();

    // Hysteresis
    settings.pkpk = pkpk;
    settings.symmetry_p = 100.;
    settings.offset = offset;
    for period in hyst_periods {
        settings.period = Duration::from_secs_f64(period);
        let mut file_path = folder.clone();
        file_path.push(filename(settings));
        if file_path.exists() {
            continue;
        }
        println!("Running {}", filename(settings));
        let aq = aqd.aquire_n_waves(settings, num_samples).await?;
        let writer = BufWriter::new(std::fs::File::create(file_path)?);
        aq.write_to(writer)?;
    }

    // Ramp
    settings.pkpk = pkpk;
    settings.offset = offset;
    for ramp_time in ramp_times {
        let ramp_dur = Duration::from_secs_f64(ramp_time);
        let ramp_rest_dur = Duration::from_secs_f64(ramp_rest_time);
        settings.set_ramp_time(ramp_dur, ramp_rest_dur);
        let mut file_path = folder.clone();
        file_path.push(filename(settings));
        if file_path.exists() {
            continue;
        }
        println!("Running {}", filename(settings));
        let aq = aqd.aquire_n_waves(settings, num_samples).await?;
        let writer = BufWriter::new(std::fs::File::create(file_path)?);
        aq.write_to(writer)?;
    }

    aqd.stop_wavegen().await?;
    Ok(())
}

fn filename(settings: WavegenSettings) -> String {
    format!(
        "trap_{:.2}s_{:.2}v_{:.2}p.dat",
        settings.period.as_secs_f64(),
        settings.pkpk,
        settings.symmetry_p,
    )
}
