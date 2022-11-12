use std::io;
use std::time::Duration;
use std::thread;
use std::time::Instant;

use rppal::gpio::{Gpio, IoPin, Mode, PullUpDown, Level};
use thread_priority::{ThreadBuilder, ThreadPriority};

const HUMIDITY_HIGH_BYTE_INDEX: usize = 0;
const HUMIDITY_LOW_BYTE_INDEX: usize = 1;
const TEMPERATURE_HIGH_BYTE_INDEX: usize = 2;
const TEMPERATURE_LOW_BYTE_INDEX: usize = 3;
const CHECKSUM_BYTE_INDEX: usize = 4;

const NUM_DATA_BYTES: usize = 5;
const NUM_DATA_BITS: usize = NUM_DATA_BYTES * 8;

const PULSE_TIMEOUT: Duration = Duration::new(0, 200_000);

#[derive(Debug)]
pub struct DHT22 {
    gpio: Gpio,
    pin: u8,
}

impl DHT22 {
    pub fn new(gpio_pin: u8) -> io::Result<DHT22> {
        let gpio = match Gpio::new() {
            Ok(gpio) => gpio,
            Err(gpio_error) => {
                return Err(io::Error::new(io::ErrorKind::Other, gpio_error));
            }
        };

        Ok(DHT22 { gpio, pin: gpio_pin })
    }

    pub fn dht22_read_temperature(&mut self) -> io::Result<f32> {
        let read_bytes = self.dht22_read()?;

        let temperature: u16 = (((read_bytes[TEMPERATURE_HIGH_BYTE_INDEX] & 0x7F) as u16) << 8) | read_bytes[TEMPERATURE_LOW_BYTE_INDEX] as u16;
        let temperature = (temperature as f32) * 0.1;

        /* Temperature is negative if MSB in temperature is 1 */
        if read_bytes[TEMPERATURE_HIGH_BYTE_INDEX] & 0x80 != 0 {
            return Ok(-temperature);
        }

        Ok(temperature)
    }

    pub fn dht22_read_humidity(&mut self) -> io::Result<f32> {
        let read_bytes = self.dht22_read()?;

        let humidity: u16 = ((read_bytes[HUMIDITY_HIGH_BYTE_INDEX] as u16) << 8) | read_bytes[HUMIDITY_LOW_BYTE_INDEX] as u16;
        let humidity = (humidity as f32) * 0.1;

        Ok(humidity)
    }

    fn dht22_read(&mut self) -> io::Result<[u8; 5]> {
        let mut pin = match self.gpio.get(self.pin) {
            Ok(pin) => pin.into_io(Mode::Output),
            Err(gpio_error) => {
                return Err(io::Error::new(io::ErrorKind::Other, gpio_error));
            }
        };

        /* Spawn a thread with high priority to handle timing critical
           GPIO actions */
        let read_thread = ThreadBuilder::default()
            .name("DHT22ReadThread")
            .priority(ThreadPriority::Max)
            .spawn(move |_result| {
                /* Set pullup as the sensor will actively drive the data line
                   low */
                pin.set_pullupdown(PullUpDown::PullUp);

                /* Set high to establish bus idle */
                pin.set_mode(Mode::Output);
                pin.set_high();
                thread::sleep(Duration::from_millis(1));

                /* Pull down to send start signal. Datasheet says 1-10 ms, but
                   at least 1 ms */
                pin.set_low();
                thread::sleep(Duration::from_micros(1100));

                /* Set high before switching to input, to avoid faulty read
                   of previous low output from the GPIO pin */
                pin.set_high();

                /* Set to input. Since pull up resistors are enabled this sets
                   the data line high. Datasheet says sensor should leave it
                   high for 20 - 40 us */
                pin.set_mode(Mode::Input);
                measure_pulse(&mut pin, Level::High, PULSE_TIMEOUT);


                /* Sensor should then pull data low for 80 us */
                let pulse_length_low = measure_pulse(&mut pin, Level::Low, PULSE_TIMEOUT);
                if pulse_length_low < Duration::from_micros(70) || pulse_length_low > Duration::from_micros(90) {
                    return Err(io::Error::from(io::ErrorKind::TimedOut));
                }

                /* And then high for 80 us */
                let pulse_length_high = measure_pulse(&mut pin, Level::High, PULSE_TIMEOUT);
                if pulse_length_high < Duration::from_micros(70) || pulse_length_high > Duration::from_micros(90) {
                    return Err(io::Error::from(io::ErrorKind::TimedOut));
                }

                /* Read pulse lengths. Each bit should start with a ~50 us
                   high pulse, followed by ~25 us or ~70 us low pulse. If the
                   low pulse is ~25 us, it represents a bit with value 0. If the
                   low pulse is ~70 us, it represents a bit with value 1.

                   Since this section is timing critical, bit validation will
                   happen after reading the data line. */
                let mut bit_transfer_start_pulse_durations = [Duration::new(0, 0); NUM_DATA_BITS];
                let mut pulse_durations = [Duration::new(0, 0); NUM_DATA_BITS];
                for i in 0..NUM_DATA_BITS {
                    bit_transfer_start_pulse_durations[i] = measure_pulse(&mut pin, Level::Low, PULSE_TIMEOUT);
                    pulse_durations[i] = measure_pulse(&mut pin, Level::High, PULSE_TIMEOUT);
                }

                /* Validate pulse lengths, evaluate bit values and merge them
                   into bytes */
                let mut bytes = [0u8; NUM_DATA_BYTES];
                for i in 0..NUM_DATA_BITS {
                    if pulse_durations[i] > PULSE_TIMEOUT {
                        return Err(io::Error::from(io::ErrorKind::TimedOut));
                    }

                    let bit_value = (pulse_durations[i] > bit_transfer_start_pulse_durations[i]) as u8;

                    /* Data bits comes in with MSB first */
                    bytes[i / 8] |= bit_value << (7 - (i % 8));
                }

                /* Check checksum */
                if bytes[CHECKSUM_BYTE_INDEX] != (bytes[HUMIDITY_HIGH_BYTE_INDEX] as u16 + bytes[HUMIDITY_LOW_BYTE_INDEX] as u16 + bytes[TEMPERATURE_HIGH_BYTE_INDEX] as u16 + bytes[TEMPERATURE_LOW_BYTE_INDEX] as u16) as u8 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "Checksum failed"));
                }

                Ok(bytes)
            })?;

        read_thread.join().expect("should be able to join thread")
    }
}

/* Measure the length of a pulse of given logic level. The mode of the
       GPIO pin MUST be set to input for this to work */
fn measure_pulse(pin: &mut IoPin, level: Level, timeout: Duration) -> Duration {
    let now = Instant::now();

    while pin.read() == level {
        if now.elapsed() > timeout {
            break;
        }
    }

    now.elapsed()
}
