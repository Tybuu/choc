#![no_std]
#![no_main]

use bruh78::cirque::{AbsoluteDataPacket, TrackPad};
use bruh78::keys::{Keys, DEBOUNCE_TIME};
use bruh78::matrix::Matrix;
use core::mem;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use defmt_rtt as _;
use embassy_futures::join::{join, join3};
use embassy_futures::select::select;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pin, Pull};
use embassy_nrf::gpiote::{Channel, InputChannel, InputChannelPolarity};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::twim::{Config as I2cConfig, Twim};
use embassy_nrf::usb::vbus_detect::HardwareVbusDetect;
use embassy_nrf::usb::{self, Driver};
use embassy_nrf::{bind_interrupts, peripherals, twim};
use embassy_time::{self, Duration, Instant, Timer};
use embassy_usb::class::hid::{HidWriter, State};
use embassy_usb::{Builder, Handler};
use nrf_softdevice::ble::gatt_server;
use usbd_hid::descriptor::{MouseReport, SerializedDescriptor};
// time driver
use panic_probe as _;

use defmt::*;
use embassy_executor::Spawner;
use nrf_softdevice::ble::advertisement_builder::{
    Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload, ServiceList,
};
use nrf_softdevice::ble::{
    gatt_client, peripheral, set_address, set_whitelist, Address, AddressType,
};
use nrf_softdevice::{ble, raw, RawError, Softdevice};

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

#[nrf_softdevice::gatt_service(uuid = "9e7312e0-2354-11eb-9f10-fbc30a62cf38")]
struct KeyClient {
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf38", read, write, notify)]
    state: u64,
}
#[nrf_softdevice::gatt_server]
struct Server {
    key_client: KeyClient,
}

bind_interrupts!(struct Irqs {
    SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0 => twim::InterruptHandler<peripherals::TWISPI0>;
    USBD => usb::InterruptHandler<peripherals::USBD>;
    POWER_CLOCK => usb::vbus_detect::InterruptHandler;
});

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, peripherals::USBD, HardwareVbusDetect>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    let mut nrf_config = embassy_nrf::config::Config::default();
    // nrf_config.gpiote_interrupt_priority = Priority::P2;
    // nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);
    let mut led = Output::new(p.P0_15, Level::Low, OutputDrive::Standard);

    let mut power = Output::new(p.P0_13, Level::High, OutputDrive::Standard);
    Timer::after_millis(5).await;
    let mut iConfig = I2cConfig::default();
    iConfig.sda_pullup = true;
    iConfig.scl_pullup = true;
    let mut i2c = Twim::new(p.TWISPI0, Irqs, p.P0_24, p.P0_22, iConfig);
    let mut trackpad = TrackPad::new(&mut i2c).await;
    trackpad.sleep(true).await;
    let mut columns = [
        Output::new(p.P0_09.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_10.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_11.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_15.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_02.degrade(), Level::Low, OutputDrive::Standard),
    ];

    let rows = [
        Input::new(p.P1_00.degrade(), Pull::Down),
        Input::new(p.P0_11.degrade(), Pull::Down),
        Input::new(p.P1_04.degrade(), Pull::Down),
        Input::new(p.P1_06.degrade(), Pull::Down),
    ];

    let mut matrix = Matrix::new(columns, rows);

    let driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    // spawner.spawn(logger_task(driver)).unwrap();
    let mut config = embassy_usb::Config::new(0xa59, 0xa59);
    config.manufacturer = Some("bad");
    config.product = Some("bad");
    config.max_power = 500;
    config.max_packet_size_0 = 64;
    //
    // // Create embassy-usb DeviceBuilder using the driver and config.
    // // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];
    let mut device_handler = MyDeviceHandler::new();

    let mut mouse_state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.handler(&mut device_handler);

    // Create classes on the builder.
    let mouse_config = embassy_usb::class::hid::Config {
        report_descriptor: MouseReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 5,
    };

    let mut mouse_writer = HidWriter::<_, 5>::new(&mut builder, &mut mouse_state, mouse_config);

    // Build the builder.
    let mut usb = builder.build();
    let usb_fut = usb.run();

    let main_loop = async {
        let mut key_state = 0u64;
        let mut key_states = [Debouncer::default(); 18];
        Timer::after_secs(1).await;
        loop {
            let mut current_state = 0u64;
            let mut states = [[false; 5]; 4];
            matrix.scan(&mut states).await;
            let iter = states.iter().flatten().into_iter();
            let mut index = 0;
            for state in iter {
                if index < 18 {
                    key_states[index].update_buf(*state);
                    index += 1;
                }
            }
            for i in 0..key_states.len() {
                if key_states[i].is_pressed() {
                    current_state |= 1 << i;
                } else {
                    current_state &= !(1 << i);
                }
            }
            if key_state != current_state {
                key_state = current_state;
            }
            Timer::after_micros(5).await;
        }
    };
    let mouse_loop = async {
        Timer::after_secs(1).await;
        let mut time = Instant::now();
        let def = 10;
        let mut sleep_time = def;
        loop {
            let data = trackpad.get_relative().await;
            match data {
                Some((x, y, buttons)) => {
                    log::info!("x: {}, y: {}", x, y);
                    let rep = MouseReport {
                        buttons,
                        x: y,
                        y: x,
                        wheel: 0,
                        pan: 0,
                    };
                    let data = ((((y as u16) << 8) | x as u16) as u64) << 32;
                    // match server.key_client.state_notify(&conn, &data) {
                    //     Ok(_) => info!("report sent!"),
                    //     Err(e) => error!("{:?}", e),
                    // }
                    mouse_writer.write_serialize(&rep).await;
                    time = Instant::now();
                    sleep_time = def;
                }
                None => {
                    if time.elapsed() >= Duration::from_millis(5000) {
                        sleep_time = 1000;
                    }
                    info!("No report!");
                }
            }
            Timer::after_millis(sleep_time).await;
        }
    };
    join(usb_fut, mouse_loop).await;
    // mouse_loop.await;
}

#[derive(Copy, Clone, Debug)]
struct Debouncer {
    state: bool,
    debounced: Option<Instant>,
}

impl Debouncer {
    const fn default() -> Debouncer {
        Self {
            state: false,
            debounced: None,
        }
    }
    /// Returns the pressed status of the position
    fn is_pressed(&self) -> bool {
        self.state
    }

    /// Updates the buf of the key. Updating the buf will also update
    /// the value returned from the is_pressed function
    fn update_buf(&mut self, buf: bool) {
        match self.debounced {
            Some(time) => {
                if time.elapsed() > Duration::from_millis(DEBOUNCE_TIME) {
                    self.debounced = None;
                }
            }
            None => {
                if buf != self.state {
                    self.debounced = Some(Instant::now());
                    self.state = buf;
                }
            }
        }
    }
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
