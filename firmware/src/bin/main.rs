//! This example test the RP Pico on board LED.
//!
//! It does not work with the RP Pico W board. See wifi_blinky.rs.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use bruh78::codes::KeyCodes;
use bruh78::config::load_callum;
use bruh78::descriptor::{BufferReport, KeyboardReportNKRO};
use bruh78::keys::Keys;
use bruh78::report::Report;
use cortex_m::delay::Delay;
use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::{self, join, join3, join4};
use embassy_futures::yield_now;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pin, Pull};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::peripherals::USBD;
use embassy_nrf::usb::vbus_detect::{HardwareVbusDetect, VbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};
use nrf_softdevice as _;

use embassy_nrf::usb::Driver;
use embassy_usb::class::hid::{HidReader, HidReaderWriter, HidWriter, State};
use embassy_usb::{Builder, Config, Handler};
use embedded_hal::digital::{InputPin, OutputPin};
use usbd_hid::descriptor::{KeyboardReport, KeyboardUsage, SerializedDescriptor};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    POWER_CLOCK => usb::vbus_detect::InterruptHandler;
});

static MUX: Mutex<CriticalSectionRawMutex, [u8; 3]> = Mutex::new([0u8; 3]);

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USBD, HardwareVbusDetect>) {
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

    // Create embassy-usb Config
    let mut config = Config::new(0xa55, 0xa44);
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
    let mut slave_state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    let key_config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReportNKRO::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 32,
    };
    let slave_config = embassy_usb::class::hid::Config {
        report_descriptor: BufferReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };

    let mut key_writer = HidWriter::<_, 29>::new(&mut builder, &mut key_state, key_config);
    let s_hid = HidReaderWriter::<_, 4, 1>::new(&mut builder, &mut slave_state, slave_config);

    let (mut s_reader, _) = s_hid.split();

    builder.handler(&mut device_handler);

    let mut usb = builder.build();
    let usb_fut = usb.run();
    let mut columns = [
        Output::new(p.P1_00.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_11.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_04.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_06.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_09.degrade(), Level::Low, OutputDrive::Standard),
    ];

    let rows = [
        Input::new(p.P0_02.degrade(), Pull::Down),
        Input::new(p.P1_15.degrade(), Pull::Down),
        Input::new(p.P1_11.degrade(), Pull::Down),
        Input::new(p.P0_10.degrade(), Pull::Down),
    ];

    let mut keys = Keys::<39>::default();
    load_callum(&mut keys);
    let mut report = Report::default();
    let main_loop = async {
        loop {
            for i in 0..columns.len() {
                columns[i].set_high();
                for j in 0..rows.len() {
                    if j == 3 {
                        if i > 1 {
                            let index = j * 5 + i - 2;
                            keys.update_buf(index, rows[j].is_high());
                        }
                    } else {
                        let index = j * 5 + i;
                        keys.update_buf(index, rows[j].is_high());
                    }
                }
                columns[i].set_low();
            }
            let mut slave_buf = [0u8; 3];
            let slave_keys = MUX.lock().await;
            slave_buf = *slave_keys;
            drop(slave_keys);
            for i in 0..21 {
                let a_idx = (i / 8) as usize;
                let b_idx = i % 8;
                let val = (slave_buf[a_idx] >> b_idx) & 1;
                keys.update_buf(i + 18, val != 0);
            }
            match report.generate_report(&mut keys) {
                (Some(rep), _) => key_writer.write_serialize(rep).await.unwrap(),
                (None, _) => {}
            }
            yield_now().await;
        }
    };

    let slave_loop = async {
        loop {
            let mut buf = [0u8; 32];
            s_reader.read(&mut buf).await;
            if buf[0] == 5 {
                let mut keys = MUX.lock().await;
                (*keys)[0] = buf[1];
                (*keys)[1] = buf[2];
                (*keys)[2] = buf[3];
                drop(keys);
            }
            yield_now().await;
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
    join4(usb_fut, main_loop, led_loop, slave_loop).await;
    // join(main_loop, led_loop).await;
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
