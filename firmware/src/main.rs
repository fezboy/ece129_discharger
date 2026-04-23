#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::i2c::{Error, I2c};
use embassy_stm32::{bind_interrupts, dma, i2c, peripherals};
use embassy_stm32::{ gpio::{Level, Output, Speed}, };
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306Async};


bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    GPDMA1_CHANNEL4 => dma::InterruptHandler<peripherals::GPDMA1_CH4>;
    GPDMA1_CHANNEL5 => dma::InterruptHandler<peripherals::GPDMA1_CH5>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Hello World!");

    let mut led = Output::new(p.PA5, Level::High, Speed::Low);

    let mut i2c = I2c::new(
        p.I2C1,
        p.PB6,
        p.PB7,
        p.GPDMA1_CH4,
        p.GPDMA1_CH5,
        Irqs,
        Default::default(),
    );

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306Async::new(interface, DisplaySize128x32, DisplayRotation::Rotate0).into_terminal_mode();

    display.init().await.unwrap();
    let _ = display.clear().await;

    let _ = display.write_str("Hello Rust!").await;

    let mut data = [0u8; 1];

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(500).await;

        info!("low");
        led.set_low();
        Timer::after_millis(500).await;
    }
}
