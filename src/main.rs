mod power_automate;

use std::time::Duration;

use crate::power_automate::PowerAutomate;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let pa = PowerAutomate::new();
    let resp = pa.cancel_box("title", "this is a cancel box", Duration::from_secs(5)).await?;
    println!("{resp:?}");
    let resp = pa.message_box("Title", "this is a message box").await?;
    println!("{resp:?}");
    Ok(())
}
