#![no_std]
#![no_main]

use core::mem;

use bruh78::battery::BatteryVoltage;
use bruh78::bond::Bonder;
use bruh78::config::load_colemak;
use bruh78::keys::Keys;
use bruh78::matrix::Matrix;
use bruh78::report::Report;
use bruh78::split::central::{BleCentral, Server};
use bruh78::split::link::Link;
use bruh78::storage::{Storage, NRF_FLASH_RANGE};
use defmt::{info, *};
use embassy_executor::Spawner;
use embassy_futures::select::select4;
use embassy_futures::select::{select, Either};
use embassy_nrf::gpio::Pin;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::saadc::{Gain, Saadc};
use embassy_nrf::{bind_interrupts, saadc};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as SyncChannel;
use embassy_sync::mutex::Mutex;
// time driver
use embassy_time::Timer;
use nrf_softdevice::ble::{set_address, Address, AddressType};
use nrf_softdevice::{raw, Flash, Softdevice};

use defmt_rtt as _; // global logger
use embassy_nrf as _; // time driver
use panic_probe as _;
use static_cell::StaticCell;

static BONDER: StaticCell<Bonder<Flash>> = StaticCell::new();
#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

bind_interrupts!(struct Irqs {
    SAADC => embassy_nrf::saadc::InterruptHandler;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = Priority::P2;
    nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);

    interrupt::SAADC.set_priority(interrupt::Priority::P3);

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
            p_value: b"TybeastL" as *const u8 as _,
            current_len: 8,
            max_len: 8,
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

    let mut led = Output::new(p.P0_15, Level::Low, OutputDrive::Standard);
    let sd = Softdevice::enable(&config);

    let addr = Address::new(
        AddressType::RandomStatic,
        [0x72u8, 0x72u8, 0x72u8, 0x72u8, 0x72u8, 0b11111111u8],
    );
    set_address(sd, &addr);
    info!("storage bout to init");
    let server = unwrap!(Server::new(sd, "12345678"));
    unwrap!(spawner.spawn(softdevice_task(sd)));
    let storage: Mutex<CriticalSectionRawMutex, Storage<Flash, u32>> =
        Mutex::new(Storage::init(Flash::take(&sd), NRF_FLASH_RANGE).await);
    info!("storage init");
    let mut res = (storage.lock().await);
    res.clear().await;
    info!("Clear finished!");
    loop {}
}
