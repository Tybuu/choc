#![no_std]
#![no_main]

use bruh78::keys::{Keys, DEBOUNCE_TIME};
use bruh78::matrix::Matrix;
use core::mem;
use core::ptr::NonNull;
use defmt_rtt as _;
use embassy_futures::join::join;
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
        ..Default::default()
    };

    let self_addr = Address::new(
        AddressType::RandomStatic,
        [0x66u8, 0x66u8, 0x66u8, 0x66u8, 0x66u8, 0b11111111u8],
    );
    let pair_addr = Address::new(
        AddressType::RandomStatic,
        [0x72u8, 0x72u8, 0x72u8, 0x72u8, 0x72u8, 0u8],
    );
    let sd = Softdevice::enable(&config);
    set_address(sd, &self_addr);
    // set_whitelist(sd, &[pair_addr]).unwrap();
    let server = unwrap!(Server::new(sd));
    unwrap!(spawner.spawn(softdevice_task(sd)));

    static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
        .services_128(
            ServiceList::Complete,
            &[0x9e7312e0_2354_11eb_9f10_fbc30a62cf38_u128.to_le_bytes()],
        )
        .full_name("TybeastR")
        .build();

    static SCAN_DATA: [u8; 0] = [];

    let mut led = Output::new(p.P0_15, Level::Low, OutputDrive::Standard);

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
    loop {
        let config = peripheral::Config::default();
        let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };
        let conn = peripheral::advertise_connectable(sd, adv, &config)
            .await
            .unwrap();

        info!("advertising done!");

        let mut key_state = 0u32;
        let mut key_states = [Debouncer::default(); 18];
        let main_loop = async {
            Timer::after_secs(1).await;
            loop {
                let mut current_state = 0u32;
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
                    match server.key_client.state_notify(&conn, &key_state) {
                        Ok(_) => info!("report sent"),
                        Err(e) => error!("{:?}", e),
                    }
                }
                Timer::after_micros(5).await;
            }
        };
        let e = gatt_server::run(&conn, &server, |_| {});

        join(e, main_loop).await;
    }
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
