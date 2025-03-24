#![no_std]
#![no_main]

use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicU32};
use core::{mem, slice};

use bruh78::battery::BatteryVoltage;
use bruh78::central::Server;
use bruh78::config::load_callum;
use bruh78::descriptor::KeyboardReportNKRO;
use bruh78::keys::Keys;
use bruh78::matrix;
// use bruh78::matrix::Matrix;
use bruh78::report::Report;
use defmt::{info, *};
use embassy_executor::Spawner;
use embassy_futures::select::{select, select3};
use embassy_futures::select::{select4, select_array};
use embassy_nrf::gpio::Pin;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::gpiote::Channel;
use embassy_nrf::gpiote::InputChannel;
use embassy_nrf::gpiote::InputChannelPolarity;
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::saadc::{self, Gain, Saadc};
use embassy_nrf::usb::{self, Driver};
use embassy_nrf::{self as _, bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel as SyncChannel;
use embassy_sync::mutex::Mutex;
// time driver
use embassy_time::Timer;
use embedded_hal::digital::{InputPin, OutputPin};
use nrf_softdevice::ble::advertisement_builder::{
    AdvertisementDataType, Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload,
    ServiceList, ServiceUuid16,
};
use nrf_softdevice::ble::gatt_server::builder::ServiceBuilder;
use nrf_softdevice::ble::gatt_server::characteristic::{
    Attribute, Metadata, Presentation, Properties,
};
use nrf_softdevice::ble::gatt_server::{CharacteristicHandles, RegisterError, WriteOp};
use nrf_softdevice::ble::security::SecurityHandler;
use nrf_softdevice::ble::{
    central, gatt_client, gatt_server, peripheral, set_address, Address, AddressType, Connection,
    Phy, SecurityMode, TxPower, Uuid,
};
use nrf_softdevice::{raw, RawError, Softdevice};

use defmt_rtt as _; // global logger
use embassy_nrf as _; // time driver
use panic_probe as _;
use usbd_hid::descriptor::SerializedDescriptor;
const UPDATE: AtomicBool = AtomicBool::new(false);

#[nrf_softdevice::gatt_client(uuid = "9e7312e0-2354-11eb-9f10-fbc30a62cf38")]
struct KeyClient {
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf38", read, write, notify)]
    state: u32,
}

#[nrf_softdevice::gatt_service(uuid = "9e7312e0-2354-11eb-9f10-fbc30a62cf38")]
struct KeyService {
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf38", read, write, notify)]
    state: u32,
}

#[embassy_executor::task]
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    sd.run().await
}

struct HidSecurityHandler {}

impl SecurityHandler for HidSecurityHandler {}

bind_interrupts!(struct Irqs {
    SAADC => saadc::InterruptHandler;
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
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 64 }),
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
            p_value: b"HelloRust" as *const u8 as _,
            current_len: 9,
            max_len: 9,
            write_perm: unsafe { mem::zeroed() },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
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
    let server = unwrap!(Server::new(sd, "12345678"));
    unwrap!(spawner.spawn(softdevice_task(sd)));

    static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
        .services_16(
            ServiceList::Incomplete,
            &[
                ServiceUuid16::BATTERY,
                ServiceUuid16::HUMAN_INTERFACE_DEVICE,
            ],
        )
        .full_name("HelloRust")
        // Change the appearance (icon of the bluetooth device) to a keyboard
        .raw(AdvertisementDataType::APPEARANCE, &[0xC1, 0x03])
        .build();

    static SCAN_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .services_16(
            ServiceList::Incomplete,
            &[
                ServiceUuid16::DEVICE_INFORMATION,
                ServiceUuid16::BATTERY,
                ServiceUuid16::HUMAN_INTERFACE_DEVICE,
            ],
        )
        .build();

    static SEC: HidSecurityHandler = HidSecurityHandler {};

    let mut columns = [
        Output::new(p.P1_00.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_11.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_04.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P1_06.degrade(), Level::Low, OutputDrive::Standard),
        Output::new(p.P0_09.degrade(), Level::Low, OutputDrive::Standard),
    ];

    let mut rows = [
        InputChannel::new(
            p.GPIOTE_CH0.degrade(),
            Input::new(p.P0_02.degrade(), Pull::Down),
            InputChannelPolarity::LoToHi,
        ),
        InputChannel::new(
            p.GPIOTE_CH1.degrade(),
            Input::new(p.P1_15.degrade(), Pull::Down),
            InputChannelPolarity::LoToHi,
        ),
        InputChannel::new(
            p.GPIOTE_CH2.degrade(),
            Input::new(p.P1_11.degrade(), Pull::Down),
            InputChannelPolarity::LoToHi,
        ),
        InputChannel::new(
            p.GPIOTE_CH3.degrade(),
            Input::new(p.P0_10.degrade(), Pull::Down),
            InputChannelPolarity::LoToHi,
        ),
    ];

    let mut keys = Keys::<39>::default();
    load_callum(&mut keys);

    let mut battery_channel = saadc::ChannelConfig::single_ended(saadc::VddhDiv5Input);
    battery_channel.gain = Gain::GAIN1_2;
    let mut saadc = Saadc::new(p.SAADC, Irqs, saadc::Config::default(), [battery_channel]);

    let mut battery = BatteryVoltage::new(&mut saadc, 0).await;

    let mut channel = SyncChannel::<CriticalSectionRawMutex, u32, 10>::new();
    let tx = channel.sender();
    let rx = channel.receiver();
    // let mut matrix = Matrix::new(columns, rows);
    let mut report = Report::default();
    loop {
        info!("start loop");
        let pair_addr = Address::new(
            AddressType::RandomStatic,
            [0x66u8, 0x66u8, 0x66u8, 0x66u8, 0x66u8, 0b11111111u8],
        );
        let peer_addr = [&pair_addr];
        let mut peer_config = central::ConnectConfig::default();
        peer_config.scan_config.whitelist = Some(&peer_addr);
        peer_config.conn_params.min_conn_interval = 6;
        peer_config.conn_params.max_conn_interval = 12;

        let peer_conn = central::connect(sd, &peer_config).await.unwrap();

        info!("past the connection");

        let key_client: KeyClient = gatt_client::discover(&peer_conn).await.unwrap();
        info!("past the discover");

        let mut config = peripheral::Config::default();
        // config.primary_phy = Phy::M2;
        let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };

        let conn = peripheral::advertise_pairable(sd, adv, &config, &SEC)
            .await
            .unwrap();

        conn.set_conn_params(raw::ble_gap_conn_params_t {
            min_conn_interval: 6,
            max_conn_interval: 12,
            slave_latency: 0,
            conn_sup_timeout: 400,
        })
        .unwrap();
        info!("advertising done!");

        // Run the GATT server on the connection. This returns when the connection gets disconnected.
        let e = gatt_server::run(&conn, &server, |_| {});

        key_client.state_cccd_write(true).await.unwrap();
        let e2 = gatt_client::run(&peer_conn, &key_client, |event| match event {
            KeyClientEvent::StateNotification(val) => match tx.try_send(val) {
                Ok(_) => {}
                Err(_) => {}
            },
        });

        let battery_loop = async {
            loop {
                match battery.update_reading().await {
                    Some(percentage) => {
                        server.bas.battery_level_notify(&conn, percentage);
                    }
                    None => {}
                }
                Timer::after_secs(30).await;
            }
        };

        let main_loop = async {
            Timer::after_secs(2).await;
            loop {
                // for power in &mut columns {
                //     power.set_high();
                // }
                // let mut low = true;
                // for row in &mut rows {
                //     if row.is_high().unwrap() {
                //         low = false;
                //         break;
                //     }
                // }
                // if low {
                //     select_array([
                //         rows[0].wait(),
                //         rows[1].wait(),
                //         rows[2].wait(),
                //         rows[3].wait(),
                //     ])
                //     .await;
                // }
                // for power in &mut columns {
                //     power.set_low();
                // }
                let mut pressed = false;
                for i in 0..columns.len() {
                    columns[i].set_high();
                    for j in 0..rows.len() {
                        if j == 3 {
                            if i > 1 {
                                let index = j * 5 + i - 2;
                                let res = rows[j].is_high().unwrap();
                                pressed = pressed || res;
                                keys.update_buf(index, res);
                            }
                        } else {
                            let index = j * 5 + i;
                            let res = rows[j].is_high().unwrap();
                            pressed = pressed || res;
                            keys.update_buf(index, res);
                        }
                    }
                    columns[i].set_low();
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
                match report.generate_report(&mut keys) {
                    Some(rep) => {
                        let mut val = [0u8; 29];
                        val[0] = rep.modifier;
                        val[1..29].copy_from_slice(&rep.nkro_keycodes);
                        server.hid.notify(&conn, &val);
                    }
                    _ => {}
                };
                Timer::after_micros(5).await;
            }
        };

        let res = select4(e, e2, main_loop, battery_loop).await;
        // let res = select(e, main_loop).await;
    }
}
