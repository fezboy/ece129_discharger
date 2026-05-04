#![no_std]
#![no_main]

use embassy_executor::Spawner;

use defmt::{info};
use {defmt_rtt as _, panic_probe as _};

use embassy_stm32::{
    Config,
   bind_interrupts,
   interrupt,
   gpio::{self, Input, Level, Output, Pull, Speed},
   time::{Hertz, khz},
   timer::simple_pwm::{PwmPin, SimplePwm},
   exti::{self, ExtiInput},
};

use embassy_time::Timer;

bind_interrupts!(struct Irqs {
    EXTI6 => exti::InterruptHandler<interrupt::typelevel::EXTI6>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");

    let mut config = Config::default();
    {use embassy_stm32::rcc::*;
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

    Timer::after_millis(300).await;
    let pwm_pin = PwmPin::new(p.PA8, embassy_stm32::gpio::OutputType::PushPull);
    let mut pwm = SimplePwm::new(p.TIM1, Some(pwm_pin), None, None, None, khz(10), Default::default());

    let mut ch1 = pwm.ch1();
    info!("PWM Initialized!");
    info!("PWM Max duty: {}", ch1.max_duty_cycle());

    let mut driver_fault = ExtiInput::new(p.PB6, p.EXTI6, Pull::None, Irqs);
    let driver_ready = Input::new(p.PB7, Pull::None);
    let mut driver_reset = Output::new(p.PA5, Level::Low, Speed::Low);

    driver_reset.set_high();

    loop {
        if driver_fault.is_low() {
            info!("Driver has a fault!");
            continue
        }
        ch1.set_duty_cycle_fraction(1, 10);
        ch1.enable();

        if driver_ready.is_high() {
            info!("Driver ready!");
        } else {
            info!("Driver not ready!");
        }

        info!("Fault: {}, Ready: {}, Reset: {}",
            driver_fault.get_level(),
            driver_ready.get_level(),
            driver_reset.get_output_level());

        ch1.set_duty_cycle_fully_off();
        // info!("PWM duty: {}", ch1.current_duty_cycle());
        Timer::after_millis(300).await;
        ch1.set_duty_cycle_fraction(1, 4);
        // info!("PWM duty: {}", ch1.current_duty_cycle());
        Timer::after_millis(300).await;
        ch1.set_duty_cycle_fraction(1, 2);
        // info!("PWM duty: {}", ch1.current_duty_cycle());
        Timer::after_millis(300).await;
        ch1.set_duty_cycle(ch1.max_duty_cycle());
        // info!("PWM duty: {}", ch1.current_duty_cycle());
        Timer::after_millis(300).await;
    }
}
