#![no_std]
#![no_main]

use bruh78::keys::{Keys, DEBOUNCE_TIME};
use bruh78::matrix::Matrix;
use bruh78::split::peripheral::BlePeripheral;
use core::mem;
use core::ptr::NonNull;
use defmt_rtt as _;
use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pin, Pull};
use embassy_nrf::gpiote::{Channel, InputChannel, InputChannelPolarity};
use embassy_nrf::interrupt::Priority;
use embassy_time::{self, Duration, Instant, Timer};
use nrf_softdevice::ble::gatt_server;
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
    state: u32,
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf39", read, write, notify)]
    mouse_state: u16,
}

#[nrf_softdevice::gatt_server]
struct Server {
    key_client: KeyClient,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.gpiote_interrupt_priority = Priority::P2;
    nrf_config.time_interrupt_priority = Priority::P2;
    let p = embassy_nrf::init(nrf_config);

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
        conn_gatt: None,
        gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: raw::BLE_GATTS_ATTR_TAB_SIZE_DEFAULT,
        }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 3,
            central_role_count: 3,
            central_sec_count: 0,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            p_value: b"TybeastR" as *const u8 as _,
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

    let self_addr = Address::new(
        AddressType::RandomStatic,
        [0x66u8, 0x66u8, 0x66u8, 0x66u8, 0x66u8, 0b11111111u8],
    );
    let sd = Softdevice::enable(&config);
    set_address(sd, &self_addr);
    let peripheral = BlePeripheral::init(sd);
    unwrap!(spawner.spawn(softdevice_task(sd)));

    let columns = [
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
    let mut keys = Keys::<18>::default();
    loop {
        let main_loop = async {
            Timer::after_secs(1).await;
            let mut current_state = 0u32;
            loop {
                let mut states = [[false; 5]; 4];
                matrix.scan(&mut states).await;
                let iter = states.iter().flatten().into_iter();
                let mut index = 0;
                for state in iter {
                    if index < 18 {
                        keys.update_buf(index, *state);
                        index += 1;
                    }
                }
                if keys.get_states() != current_state {
                    current_state = keys.get_states();
                    peripheral.state_notify(current_state).await;
                }
                Timer::after_micros(5).await;
            }
        };
        select(peripheral.connect(), main_loop).await;
    }
}
