//! This example test the RP Pico on board LED.
//!
//! It does not work with the RP Pico W board. See wifi_blinky.rs.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use cortex_m::delay::Delay;
use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::{self, join, join3};
use embassy_futures::yield_now;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::usb::vbus_detect::{HardwareVbusDetect, VbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};

use embassy_nrf::usb::Driver;
use embassy_usb::class::hid::{HidReaderWriter, HidWriter, State};
use embassy_usb::{Builder, Config, Handler};
use usbd_hid::descriptor::{KeyboardReport, KeyboardUsage, SerializedDescriptor};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    POWER_CLOCK => usb::vbus_detect::InterruptHandler;
});

static MUX: Mutex<CriticalSectionRawMutex, [u8; 3]> = Mutex::new([0u8; 3]);

pub const NUM_KEYS: usize = 42;
#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, peripherals::USBD, HardwareVbusDetect>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    let mut led = Output::new(p.P0_15, Level::Low, OutputDrive::Standard);
    let mut state = true;
    // _spawner.spawn(logger_task(driver)).unwrap();

    // Create embassy-usb Config
    let mut config = Config::new(0xa55, 0x727);
    config.manufacturer = Some("Tybeast bruh");
    config.product = Some("Hello there");
    config.max_power = 500;
    config.max_packet_size_0 = 64;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];
    let mut device_handler = MyDeviceHandler::new();

    let mut key_state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    let key_config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 32,
    };

    let (key_r, mut key_w) =
        HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut key_state, key_config).split();

    builder.handler(&mut device_handler);

    let mut usb = builder.build();
    let usb_fut = usb.run();
    let input = Input::new(p.P0_10, Pull::Down);
    let output = Output::new(p.P1_11, Level::High, OutputDrive::Standard);
    let mut rep_sent = false;
    let main_loop = async {
        loop {
            if input.is_high() && !rep_sent {
                let mut rep = KeyboardReport::default();
                rep.keycodes[0] = KeyboardUsage::KeyboardAa as u8;
                rep_sent = true;
                key_w.write_serialize(&rep).await.unwrap();
            } else if rep_sent && input.is_low() {
                let mut rep = KeyboardReport::default();
                rep.keycodes[0] = 0x0;
                rep_sent = false;
                key_w.write_serialize(&rep).await.unwrap();
            }
            Timer::after_micros(500).await;
        }
    };

    let led_loop = async {
        let mut state = true;
        loop {
            led.set_level(state.into());
            state = !state;
            Timer::after_millis(2000).await;
        }
    };
    join3(main_loop, usb_fut, led_loop).await;
}

struct MyDeviceHandler {
    configured: AtomicBool,
}

impl MyDeviceHandler {
    fn new() -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for MyDeviceHandler {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device configured, it may now draw up to the configured current limit from Vbus."
            )
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}

fn find_order(ary: &mut [usize]) {
    let mut new_ary = [0usize; NUM_KEYS / 2];
    for i in 0..ary.len() {
        for j in 0..ary.len() {
            if ary[j as usize] == i {
                new_ary[i as usize] = j;
            }
        }
    }
    ary.copy_from_slice(&new_ary);
}
