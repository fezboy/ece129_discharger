#![no_std]
#![no_main]


use defmt::{panic, assert, *};
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_stm32::i2c::{I2c};
use embassy_stm32::{bind_interrupts, dma, i2c, peripherals, Peri};
use embassy_stm32::peripherals::USB;
use embassy_stm32::{Config, usb, gpio::{Level, Output, Speed}};
use embassy_stm32::timer::{qei, qei::{Qei}};
use embassy_time::Timer;
use embassy_stm32::time::Hertz;

use embassy_stm32::usb::{DmPin, DpPin, Driver, Instance};
use embassy_usb::class::cdc_acm::{self, CdcAcmClass};
use embassy_usb::driver::EndpointError;

use {defmt_rtt as _, panic_probe as _};
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306Async};

use heapless::{format, String, string::StringView};


bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    GPDMA1_CHANNEL4 => dma::InterruptHandler<peripherals::GPDMA1_CH4>;
    GPDMA1_CHANNEL5 => dma::InterruptHandler<peripherals::GPDMA1_CH5>;
    USB_DRD_FS => usb::InterruptHandler<peripherals::USB>;
});

const USB_MAX_PACKET_SIZE: usize = 64;

type UsbDriver<'a> = embassy_stm32::usb::Driver<'a, USB>;
type Builder<'a> = embassy_usb::Builder<'a, UsbDriver<'a>>;


fn init_usb(p_usb: Peri<'static, USB>,
    p_dp: Peri<'static, impl DpPin<USB>>,
    p_dm: Peri<'static, impl DmPin<USB>>) -> (
        CdcAcmClass<'static, UsbDriver<'static>>,
        embassy_usb::UsbDevice<'static, UsbDriver<'static>>) {
    // Create USB Driver
    let driver = Driver::new(p_usb, Irqs, p_dp, p_dm);
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Formula Slug");
    config.product = Some("Discharger");
    config.serial_number = Some("12345678");

    static mut CONFIG_DESCRIPTOR: [u8; 256] = [0; 256];
    static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
    static mut CONTROL_BUF: [u8; 64] = [0; 64];

    static mut USB_STATE: cdc_acm::State = cdc_acm::State::new();

    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut CONFIG_DESCRIPTOR},
        unsafe {&mut BOS_DESCRIPTOR},
        &mut [], // No Msos descriptors
        unsafe {&mut CONTROL_BUF},
    );


    let usb_class = CdcAcmClass::new(&mut builder, unsafe {&mut USB_STATE}, USB_MAX_PACKET_SIZE as u16);
    let usb = builder.build();
    return (usb_class, usb);
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");

    // Configure Clocks
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = None;
        config.rcc.hsi48 = Some(Hsi48Config {sync_from_usb: true}); // Needed for USB
        config.rcc.hse = Some(Hse {
            freq: Hertz(24_000_000),
            mode: HseMode::Oscillator,
        });

        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV3,
            mul: PllMul::MUL62,
            divp: Some(PllDiv::DIV2), // 250mHz
            divq: Some(PllDiv::DIV2),
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
    let p = embassy_stm32::init(config);


    let (mut usb_class, mut usb) = init_usb(p.USB, p.PA12, p.PA11);

    let usb_fut = usb.run();

    let mut led = Output::new(p.PA5, Level::High, Speed::Low);

    let i2c = I2c::new(
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


    // Set up QEI Driver
    let qei_config = qei::Config::default();
    let qei = Qei::new(p.TIM3, p.PC6, p.PA7, qei_config);

    let _ = display.write_str("Hello Rust!").await;


    let blinky_fut = async {
           loop {
        // info!("high");
        led.set_high();
        Timer::after_millis(500).await;

        // info!("low");
        led.set_low();
        Timer::after_millis(500).await;
    }};

    let echo_fut = async {
        loop {
            usb_class.wait_connection().await;
            info!("Connected!");
            let _ = echo(&mut usb_class).await;
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

async fn write_packet<'d, T: Instance + 'd>(accumulator: &mut StringView, class: &mut CdcAcmClass<'d, Driver<'d, T>>) -> Result<(), Disconnected> {
    class.write_packet(accumulator.as_bytes()).await?;
    accumulator.clear();
    Ok(())
}

async fn push_str_or_write<'d, T: Instance + 'd>(accumulator: &mut StringView, s: &[u8], class: &mut CdcAcmClass<'d, Driver<'d, T>>) -> Result<(), Disconnected> {
    assert!(accumulator.capacity() > s.len());
    if accumulator.capacity() - accumulator.len() < s.len() {
        debug!("Writing packet with length {}", accumulator.len());
        write_packet(accumulator, class).await?;
    }
    accumulator.push_str(unsafe {str::from_utf8_unchecked(s)}).expect("Received string somehow bigger than accumulator");
    Ok(())
}

async fn echo<'d, T: Instance + 'd>(class: &mut CdcAcmClass<'d, Driver<'d, T>>) -> Result<(), Disconnected> {
    let mut buf = [0; USB_MAX_PACKET_SIZE];
    let mut accumulator = String::<USB_MAX_PACKET_SIZE>::new();

    loop {
        let n = class.read_packet(&mut buf).await?;
        let mut data = &buf[..n];

        // If there are newlines, print the data!
        while let Some(pos) = data.iter().position(|&c| c == b'\n') {
            push_str_or_write(&mut accumulator, &data[..pos + 1], class).await?;
            write_packet(&mut accumulator, class).await?;
            data = &data[pos + 1..];
        }

        push_str_or_write(&mut accumulator, &data, class).await?;
        info!("Data: {:x}, len {}, total accumulated: {}", data, data.len(), accumulator.len());
    }
}
