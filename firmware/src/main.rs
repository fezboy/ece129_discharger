#![no_std]
#![no_main]

use defmt::{assert, panic, *};
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_stm32::{Config, timer::qei};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::qei::Direction;
use embassy_time::Timer;

use embassy_stm32::usb::{Driver, Instance};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::driver::EndpointError;

use ssd1306::prelude::*;
use {defmt_rtt as _, panic_probe as _};

use heapless::{String, string::StringView};

use board::{USB_MAX_PACKET_SIZE, SCREEN_WIDTH};


mod board;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");

    // Configure Clocks
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = None;
        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: true,
        }); // Needed for USB
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

    let mut board = board::Board::init(p);
    let usb_fut = board.usb.run();

    board.display.init().await.unwrap();
    let _ = board.display.clear().await;

    let _ = board.display.write_str("Hello Rust!").await;


    let mut screen_buf: String<SCREEN_WIDTH> = String::new();
    let mut direction: qei::Direction = qei::Direction::Downcounting;
    let mut count: u16 = 0;
    let screen_fut = async {
        loop {
            direction = board.qei.read_direction();
            let lastcount = count;
            count = board.qei.count();
            if count == lastcount {continue;}

            debug!("Moved {}. Current position: {}",
                  match direction {
                      Direction::Downcounting => "down",
                      Direction::Upcounting => "up",
                  },
                  count
            );


            screen_buf.clear();
            for _ in 0..count {
                screen_buf
                    .push('#')
                    .expect("Overflowed screen width [Impossible?]");
            };

            for _ in count..SCREEN_WIDTH as u16 {
                screen_buf.push(' ').expect("Overflowed screen width [Impossible?]");
            }

            board.display.set_position(0, 0).await
                .unwrap_or_else(|e| {error!("Couldn't set position! {:#?}", defmt::Debug2Format(&e))});

            board.display.write_str(screen_buf.as_str()).await
                .unwrap_or_else(|e| error!("Couldn't write to display! {:#?}", defmt::Debug2Format(&e)));

            Timer::after_millis(16).await;
        }
    };

    let blinky_fut = async {
        loop {
            // info!("high");
            board.blink_led.set_high();
            Timer::after_millis(500).await;

            // info!("low");
            board.blink_led.set_low();
            Timer::after_millis(500).await;
        }
    };

    let echo_fut = async {
        loop {
            board.usb_class.wait_connection().await;
            info!("Connected!");
            let _ = echo(&mut board.usb_class).await;
            info!("Disconnected :(");
        }
    };

    info!("Hello world!");

    join4(usb_fut, echo_fut, blinky_fut, screen_fut).await;
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

async fn write_packet<'d, T: Instance + 'd>(
    accumulator: &mut StringView,
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
    class.write_packet(accumulator.as_bytes()).await?;
    accumulator.clear();
    Ok(())
}

async fn push_str_or_write<'d, T: Instance + 'd>(
    accumulator: &mut StringView,
    s: &[u8],
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
    assert!(accumulator.capacity() > s.len());
    if accumulator.capacity() - accumulator.len() < s.len() {
        debug!("Writing packet with length {}", accumulator.len());
        write_packet(accumulator, class).await?;
    }
    accumulator
        .push_str(unsafe { str::from_utf8_unchecked(s) })
        .expect("Received string somehow bigger than accumulator");
    Ok(())
}

async fn echo<'d, T: Instance + 'd>(
    class: &mut CdcAcmClass<'d, Driver<'d, T>>,
) -> Result<(), Disconnected> {
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
        info!(
            "Data: {:x}, len {}, total accumulated: {}",
            data,
            data.len(),
            accumulator.len()
        );
    }
}
