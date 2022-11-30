mod power_automate;

use crate::power_automate::{PowerAutomate, ServerCommand};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let pa = PowerAutomate::new();
    let resp = pa
        .execute(&ServerCommand::CancelBox {
            title: "Cancel Box",
            message: "",
            time_s: 2,
        })
        .await?;
    println!("{resp:?}");
    let resp = pa
        .execute(&ServerCommand::MessageBox {
            title: "title",
            message: "message",
        })
        .await?;
    println!("{resp:?}");
    let resp = pa
        .execute(&ServerCommand::CancelBox {
            title: "Cancel Box the second",
            message: "",
            time_s: 2,
        })
        .await?;
    println!("{resp:?}");
    Ok(())
}
