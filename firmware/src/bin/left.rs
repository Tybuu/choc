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
// time driver
use embassy_time::Timer;
use nrf_softdevice::ble::{set_address, Address, AddressType};
use nrf_softdevice::{raw, Flash, Softdevice};

use defmt_rtt as _; // global logger
use embassy_nrf as _; // time driver
use panic_probe as _;
use static_cell::StaticCell;

static BONDER: StaticCell<Bonder<Flash>> = StaticCell::new();
static STORAGE: StaticCell<Storage<Flash, u32>> = StaticCell::new();

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

    let sd: &'static mut Softdevice = Softdevice::enable(&config);

    let addr = Address::new(
        AddressType::RandomStatic,
        [0x72u8, 0x72u8, 0x72u8, 0x72u8, 0x72u8, 0b11111111u8],
    );
    set_address(sd, &addr);
    let central = BleCentral::init(sd);
    unwrap!(spawner.spawn(softdevice_task(sd)));
    let storage: &'static Storage<Flash, u32> =
        STORAGE.init(Storage::init(Flash::take(&sd), NRF_FLASH_RANGE).await);

    let columns = [
        Output::new(p.P1_00.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_11.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_04.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_06.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_09.degrade(), Level::Low, OutputDrive::Standard),
    ];

    let mut rows = [
        Input::new(p.P0_02.degrade(), Pull::Down),
        Input::new(p.P1_15.degrade(), Pull::Down),
        Input::new(p.P1_11.degrade(), Pull::Down),
        Input::new(p.P0_10.degrade(), Pull::Down),
    ];

    let mut keys = Keys::<39>::default();
    load_colemak(&mut keys);

    let mut battery_channel = saadc::ChannelConfig::single_ended(p.P0_31);
    battery_channel.gain = Gain::GAIN1;
    let mut saadc = Saadc::new(p.SAADC, Irqs, saadc::Config::default(), [battery_channel]);
    let mut battery = BatteryVoltage::new(&mut saadc, 0).await;

    let channel = SyncChannel::<CriticalSectionRawMutex, u32, 10>::new();
    let tx = channel.sender();
    let rx = channel.receiver();
    let mut matrix = Matrix::new(columns, rows);
    let mut report = Report::default();

    let sd: &'static Softdevice = &*sd;

    let mut link = Link::new(tx);
    let bonder: &'static Bonder<_> = BONDER.init(Bonder::init(storage).await);

    loop {
        info!("start loop");
        let battery_loop = async {
            loop {
                match battery.update_reading().await {
                    Some(percentage) => {
                        central.battery_notify(percentage).await;
                    }
                    None => {}
                }
                Timer::after_secs(60).await;
            }
        };

        let main_loop = async {
            Timer::after_secs(2).await;
            loop {
                let mut states = [[false; 5]; 4];
                match select(matrix.scan(&mut states), rx.receive()).await {
                    Either::First(_) => {
                        states[3][0] = states[3][2];
                        states[3][1] = states[3][3];
                        states[3][2] = states[3][4];
                        let iter = states.iter().flatten().into_iter();
                        let mut index = 0;
                        for state in iter {
                            keys.update_buf_central(index, *state);
                            index += 1;
                        }
                        match rx.try_receive() {
                            Ok(val) => {
                                for i in 0..18 {
                                    let res = (val >> i) & 1 == 1;
                                    keys.update_buf(18 + i, res);
                                }
                            }
                            Err(_) => {}
                        };
                    }
                    Either::Second(val) => {
                        for i in 0..18 {
                            let res = (val >> i) & 1 == 1;
                            keys.update_buf(18 + i, res);
                        }
                    }
                }
                let (key, mouse) = report.generate_report(&mut keys);
                match key {
                    Some(rep) => {
                        central.keyboard_notify(rep).await;
                    }
                    _ => {}
                };
                match mouse {
                    Some(rep) => {
                        central.mouse_notify(rep).await;
                    }
                    None => {}
                }
                Timer::after_micros(5).await;
            }
        };

        let cen_server = central.advertise_and_connect(bonder);
        let pair_addr = Address::new(
            AddressType::RandomStatic,
            [0x66u8, 0x66u8, 0x66u8, 0x66u8, 0x66u8, 0b11111111u8],
        );

        let link_server = link.link(pair_addr);
        let _res = select4(cen_server, link_server, main_loop, battery_loop).await;
    }
}
