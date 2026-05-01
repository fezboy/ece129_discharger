use embassy_stm32::{
    Peripherals, bind_interrupts, dma,
    gpio::{Level, Output, Speed},
    i2c::{self, I2c, Master},
    mode::Async,
    peripherals::{self, TIM3},
    timer::qei::{self, Qei},
    usb::{self, Driver},
};
use embassy_usb::{
    UsbDevice,
    class::cdc_acm::{self, CdcAcmClass},
};
use ssd1306::{I2CDisplayInterface, Ssd1306Async, mode::TerminalModeAsync, prelude::*};
use static_cell::StaticCell;

pub const USB_MAX_PACKET_SIZE: usize = 64;

pub const SCREEN_WIDTH: usize = 16 * 4;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    GPDMA1_CHANNEL4 => dma::InterruptHandler<peripherals::GPDMA1_CH4>;
    GPDMA1_CHANNEL5 => dma::InterruptHandler<peripherals::GPDMA1_CH5>;
    USB_DRD_FS => usb::InterruptHandler<peripherals::USB>;
});

pub struct Board {
    pub display: Ssd1306Async<
        I2CInterface<I2c<'static, Async, Master>>,
        DisplaySize128x32,
        TerminalModeAsync,
    >,
    pub usb_class: CdcAcmClass<'static, Driver<'static, peripherals::USB>>,
    pub usb: UsbDevice<'static, Driver<'static, peripherals::USB>>,
    pub blink_led: Output<'static>,
    pub qei: Qei<'static, TIM3>,
}

impl Board {
    pub fn init(p: Peripherals) -> Self {
        // Create Display Driver
        let i2c = I2c::new(
            p.I2C1,
            p.PB6,
            p.PB7,
            p.GPDMA1_CH4,
            p.GPDMA1_CH5,
            Irqs,
            Default::default(),
        );

        // Create USB Driver
        let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("Formula Slug");
        config.product = Some("Discharger");
        config.serial_number = Some("12345678");

        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        static USB_STATE: StaticCell<cdc_acm::State> = StaticCell::new();

        let mut builder = embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [], // No Msos descriptors
            CONTROL_BUF.init([0; 64]),
        );

        let usb_class = CdcAcmClass::new(
            &mut builder,
            USB_STATE.init(cdc_acm::State::new()),
            USB_MAX_PACKET_SIZE as u16,
        );

        // Set up QEI Driver
        let mut qei_config = qei::Config::default();
        qei_config.ch1_pull = embassy_stm32::gpio::Pull::Up;
        qei_config.ch2_pull = embassy_stm32::gpio::Pull::Up;
        qei_config.auto_reload = SCREEN_WIDTH as u16;
        qei_config.mode = embassy_stm32::timer::qei::QeiMode::Mode1;
        let qei = Qei::new(p.TIM3, p.PC6, p.PA7, qei_config);

        Self {
            display: Ssd1306Async::new(
                I2CDisplayInterface::new(i2c),
                DisplaySize128x32,
                DisplayRotation::Rotate0,
            )
            .into_terminal_mode(),
            usb_class: usb_class,
            usb: builder.build(),
            blink_led: Output::new(p.PA5, Level::High, Speed::Low),
            qei: qei,
        }
    }
}
