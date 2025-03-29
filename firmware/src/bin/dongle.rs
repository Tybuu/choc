#![no_std]
#![no_main]

use core::mem;
use core::sync::atomic::{AtomicBool, Ordering};

use bruh78::battery::BatteryVoltage;
use bruh78::config::load_colemak;
use bruh78::keys::Keys;
use bruh78::matrix::Matrix;
use bruh78::report::Report;
use bruh78::split::dual::{Dual, DualMode};
use bruh78::split::link::Link;
use defmt::{info, *};
use embassy_executor::Spawner;
use embassy_futures::join;
use embassy_futures::select::{select, Either};
use embassy_futures::select::{select3, select4};
use embassy_nrf::gpio::Pin;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::saadc::{Gain, Saadc};
use embassy_nrf::usb::vbus_detect::{SoftwareVbusDetect, VbusDetect};
use embassy_nrf::{bind_interrupts, peripherals, saadc, usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as SyncChannel;
// time driver
use embassy_nrf::usb::Driver;
use embassy_time::Timer;
use embassy_usb::class::hid::{HidWriter, State};
use embassy_usb::{Builder, Handler};
use futures::future::join;
use nrf_softdevice::ble::{set_address, Address, AddressType};
use nrf_softdevice::{raw, Softdevice};

use defmt_rtt as _; // global logger
use embassy_nrf as _; // time driver
use panic_probe as _;
use static_cell::StaticCell;
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};

static VBUS: StaticCell<SoftwareVbusDetect> = StaticCell::new();

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice, vbus: &'static SoftwareVbusDetect) -> ! {
    sd.run_with_callback(|soc| match soc {
        nrf_softdevice::SocEvent::PowerUsbPowerReady => {
            vbus.ready();
        }
        nrf_softdevice::SocEvent::PowerUsbDetected => {
            vbus.detected(true);
        }
        nrf_softdevice::SocEvent::PowerUsbRemoved => {
            vbus.detected(false);
        }
        _ => {}
    })
    .await;
}

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = Priority::P2;
    nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);

    interrupt::USBD.set_priority(interrupt::Priority::P2);

    let config = nrf_softdevice::Config {
        clock: Some(raw::nrf_clock_lf_cfg_t {
            source: raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 16,
            rc_temp_ctiv: 2,
            accuracy: raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
        }),
        conn_gap: Some(raw::ble_gap_conn_cfg_t {
            conn_count: 6,
            event_length: 24,
        }),
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 256 }),
        gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: raw::BLE_GATTS_ATTR_TAB_SIZE_DEFAULT,
        }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 1,
            central_role_count: 1,
            central_sec_count: 0,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            p_value: b"TybeastDongle" as *const u8 as _,
            current_len: 13,
            max_len: 13,
            write_perm: unsafe { mem::zeroed() },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
        }),
        conn_gattc: Some(raw::ble_gattc_conn_cfg_t {
            write_cmd_tx_queue_size: 4,
        }),
        conn_gatts: Some(raw::ble_gatts_conn_cfg_t {
            hvn_tx_queue_size: 4,
        }),
        ..Default::default()
    };

    let sd: &'static mut Softdevice = Softdevice::enable(&config);

    let addr = Address::new(
        AddressType::RandomStatic,
        [0x71u8, 0x69u8, 0x72u8, 0x72u8, 0x72u8, 0b11111111u8],
    );
    set_address(sd, &addr);

    let vbus = &*VBUS.init(SoftwareVbusDetect::new(false, false));
    unwrap!(spawner.spawn(softdevice_task(sd, vbus)));

    let driver = Driver::new(p.USBD, Irqs, vbus);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xa55, 0xa44);
    config.manufacturer = Some("Tybeast Corp.");
    config.product = Some("Dongle");
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

    let mut key_writer = HidWriter::<_, 29>::new(&mut builder, &mut key_state, key_config);
    builder.handler(&mut device_handler);
    let mut usb = builder.build();
    let usb_fut = usb.run();

    let cen_channel = SyncChannel::<CriticalSectionRawMutex, u32, 10>::new();
    let cen_tx = cen_channel.sender();
    let cen_rx = cen_channel.receiver();
    let cen_link = Link::new(cen_tx);
    let cen_addr = Address::new(
        AddressType::RandomStatic,
        [0x72u8, 0x72u8, 0x72u8, 0x71u8, 0x71u8, 0b11111111u8],
    );

    let perp_channel = SyncChannel::<CriticalSectionRawMutex, u32, 10>::new();
    let perp_tx = perp_channel.sender();
    let perp_rx = perp_channel.receiver();
    let perp_link = Link::new(perp_tx);
    let perp_addr = Address::new(
        AddressType::RandomStatic,
        [0x66u8, 0x66u8, 0x66u8, 0x66u8, 0x66u8, 0b11111111u8],
    );

    let mut report = Report::default();
    let mut keys = Keys::<39>::default();
    load_colemak(&mut keys);

    let main_loop = async {
        info!("start loop");
        loop {
            let key_loop = async {
                match select(perp_rx.receive(), cen_rx.receive()).await {
                    Either::First(rep) => {
                        for i in 0..18 {
                            let res = (rep >> i) & 1 == 1;
                            keys.update_buf(i, res);
                        }
                        if let Ok(p_rep) = cen_rx.try_receive() {
                            for i in 0..18 {
                                let res = (p_rep >> i) & 1 == 1;
                                keys.update_buf(i + 18, res);
                            }
                        }
                    }
                    Either::Second(rep) => {
                        for i in 0..18 {
                            let res = (rep >> i) & 1 == 1;
                            keys.update_buf(i + 18, res);
                        }
                        if let Ok(c_rep) = perp_rx.try_receive() {
                            for i in 0..18 {
                                let res = (c_rep >> i) & 1 == 1;
                                keys.update_buf(i, res);
                            }
                        }
                    }
                };
                match report.generate_report(&mut keys) {
                    (Some(rep), _) => key_writer.write_serialize(rep).await.unwrap(),
                    _ => {}
                }
                Timer::after_micros(5).await;
            };
            select3(key_loop, perp_link.link(perp_addr), cen_link.link(cen_addr)).await;
        }
    };
    join(usb_fut, main_loop).await;
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
