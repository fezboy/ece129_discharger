#![no_std]
#![no_main]

use defmt::{panic, *};
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_stm32::i2c::{Error, I2c};
use embassy_stm32::{bind_interrupts, dma, i2c, peripherals};
use embassy_stm32::{Config, usb, gpio::{Level, Output, Speed}};
use embassy_time::Timer;
use embassy_stm32::time::Hertz;

use embassy_stm32::usb::{Driver, Instance};
use embassy_usb::class::cdc_acm::{self, CdcAcmClass};
use embassy_usb::driver::EndpointError;

use {defmt_rtt as _, panic_probe as _};
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306Async};


bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    GPDMA1_CHANNEL4 => dma::InterruptHandler<peripherals::GPDMA1_CH4>;
    GPDMA1_CHANNEL5 => dma::InterruptHandler<peripherals::GPDMA1_CH5>;
    USB_DRD_FS => usb::InterruptHandler<peripherals::USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = None;
        config.rcc.hsi48 = Some(Hsi48Config {sync_from_usb: true}); // Needed for USB
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::BypassDigital,
        });

        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV2,
            mul: PllMul::MUL125,
            divp: Some(PllDiv::DIV2), // 250mHz
            divq: None,
            divr: None,
        });

        config.rcc.ahb_pre = AHBPrescaler::DIV2;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.apb3_pre = APBPrescaler::DIV4;
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.voltage_scale = VoltageScale::Scale0;
        config.rcc.mux.usbsel = mux::Usbsel::HSI48;
    }
    info!("Hello 1!");
    let p = embassy_stm32::init(config);
    info!("Hello 2");

    // Create USB Driver
    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Formula Slug");
    config.product = Some("Discharger");
    config.serial_number = Some("12345678");

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0, 64];

    let mut state = cdc_acm::State::new();
    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    let mut usb = builder.build();
    let usb_fut = usb.run();

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

    let blinky_fut = async {
           loop {
        info!("high");
        led.set_high();
        Timer::after_millis(500).await;

        info!("low");
        led.set_low();
        Timer::after_millis(500).await;
    }};

    let echo_fut = async {
        loop {
            class.wait_connection().await;
            info!("Connected!");
            let _ = echo(&mut class).await;
            info!("Disconnected :(");
        }
    };

    info!("Hello world!");

    join3(usb_fut, echo_fut, blinky_fut).await;
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow!"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

async fn echo<'d, T: Instance + 'd>(class: &mut CdcAcmClass<'d, Driver<'d, T>>) -> Result<(), Disconnected> {
    let mut buf = [0; 64];
    loop {
        let n = class.read_packet(&mut buf).await?;
        let data = &buf[..n];
        info!("Data: {:x}", data);
        class.write_packet(data).await?
    }
}