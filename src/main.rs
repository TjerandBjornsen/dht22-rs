use dht22_rs::DHT22;

use std::error::Error;

mod cli;
use cli::DHT22Cli;
use clap::Parser;

fn main() -> Result<(), Box<dyn Error>> {
    let args = DHT22Cli::parse();

    let mut sensor = DHT22::new(16)?;

    match args.command_type {
        cli::CommandType::Temp => {
            println!("Temperature: {:#?}°C", sensor.dht22_read_temperature()?);
        },
        cli::CommandType::Humid => {
            println!("Humidity: {:#?}%", sensor.dht22_read_humidity()?);
        }
    }

    Ok(())
}
