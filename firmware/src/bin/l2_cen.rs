#![no_std]
#![no_main]

use core::{mem, slice};

use bruh78::battery::BatteryVoltage;
use bruh78::config::load_callum;
use bruh78::descriptor::KeyboardReportNKRO;
use bruh78::keys::Keys;
use bruh78::matrix::Matrix;
use bruh78::report::Report;
use defmt::{info, *};
use embassy_executor::Spawner;
use embassy_futures::select::{select, select3, Either};
use embassy_futures::select::{select4, select_array};
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::Pin;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::gpiote::Channel;
use embassy_nrf::gpiote::InputChannel;
use embassy_nrf::gpiote::InputChannelPolarity;
use embassy_nrf::interrupt::{self, InterruptExt, Priority};
use embassy_nrf::saadc::{Gain, Saadc};
use embassy_nrf::usb::{self, Driver};
use embassy_nrf::{self as _, bind_interrupts, peripherals, saadc};
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
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};
const DEVICE_INFORMATION: Uuid = Uuid::new_16(0x180a);
const BATTERY_SERVICE: Uuid = Uuid::new_16(0x180f);

const BATTERY_LEVEL: Uuid = Uuid::new_16(0x2a19);
const MODEL_NUMBER: Uuid = Uuid::new_16(0x2a24);
const SERIAL_NUMBER: Uuid = Uuid::new_16(0x2a25);
const FIRMWARE_REVISION: Uuid = Uuid::new_16(0x2a26);
const HARDWARE_REVISION: Uuid = Uuid::new_16(0x2a27);
const SOFTWARE_REVISION: Uuid = Uuid::new_16(0x2a28);
const MANUFACTURER_NAME: Uuid = Uuid::new_16(0x2a29);
const PNP_ID: Uuid = Uuid::new_16(0x2a50);

const HID_INFO: Uuid = Uuid::new_16(0x2a4a);
const REPORT_MAP: Uuid = Uuid::new_16(0x2a4b);
const HID_CONTROL_POINT: Uuid = Uuid::new_16(0x2a4c);
const HID_REPORT: Uuid = Uuid::new_16(0x2a4d);
const PROTOCOL_MODE: Uuid = Uuid::new_16(0x2a4e);

// Main items
pub const HIDINPUT: u8 = 0x80;
pub const HIDOUTPUT: u8 = 0x90;
pub const FEATURE: u8 = 0xb0;
pub const COLLECTION: u8 = 0xa0;
pub const END_COLLECTION: u8 = 0xc0;

// Global items
pub const USAGE_PAGE: u8 = 0x04;
pub const LOGICAL_MINIMUM: u8 = 0x14;
pub const LOGICAL_MAXIMUM: u8 = 0x24;
pub const PHYSICAL_MINIMUM: u8 = 0x34;
pub const PHYSICAL_MAXIMUM: u8 = 0x44;
pub const UNIT_EXPONENT: u8 = 0x54;
pub const UNIT: u8 = 0x64;
pub const REPORT_SIZE: u8 = 0x74; //bits
pub const REPORT_ID: u8 = 0x84;
pub const REPORT_COUNT: u8 = 0x94; //bytes
pub const PUSH: u8 = 0xa4;
pub const POP: u8 = 0xb4;

// Local items
pub const USAGE: u8 = 0x08;
pub const USAGE_MINIMUM: u8 = 0x18;
pub const USAGE_MAXIMUM: u8 = 0x28;
pub const DESIGNATOR_INDEX: u8 = 0x38;
pub const DESIGNATOR_MINIMUM: u8 = 0x48;
pub const DESIGNATOR_MAXIMUM: u8 = 0x58;
pub const STRING_INDEX: u8 = 0x78;
pub const STRING_MINIMUM: u8 = 0x88;
pub const STRING_MAXIMUM: u8 = 0x98;
pub const DELIMITER: u8 = 0xa8;

const KEYBOARD_ID: u8 = 0x01;

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

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum VidSource {
    BluetoothSIG = 1,
    UsbIF = 2,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PnPID {
    pub vid_source: VidSource,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_version: u16,
}

#[derive(Debug, Default, defmt::Format)]
pub struct DeviceInformation {
    pub manufacturer_name: Option<&'static str>,
    pub model_number: Option<&'static str>,
    pub serial_number: Option<&'static str>,
    pub hw_rev: Option<&'static str>,
    pub fw_rev: Option<&'static str>,
    pub sw_rev: Option<&'static str>,
}

pub struct DeviceInformationService {}

impl DeviceInformationService {
    pub fn new(
        sd: &mut Softdevice,
        pnp_id: &PnPID,
        info: DeviceInformation,
    ) -> Result<Self, RegisterError> {
        let mut sb = ServiceBuilder::new(sd, DEVICE_INFORMATION)?;

        Self::add_pnp_characteristic(&mut sb, pnp_id)?;
        Self::add_opt_str_characteristic(&mut sb, MANUFACTURER_NAME, info.manufacturer_name)?;
        Self::add_opt_str_characteristic(&mut sb, MODEL_NUMBER, info.model_number)?;
        Self::add_opt_str_characteristic(&mut sb, SERIAL_NUMBER, info.serial_number)?;
        Self::add_opt_str_characteristic(&mut sb, HARDWARE_REVISION, info.hw_rev)?;
        Self::add_opt_str_characteristic(&mut sb, FIRMWARE_REVISION, info.fw_rev)?;
        Self::add_opt_str_characteristic(&mut sb, SOFTWARE_REVISION, info.sw_rev)?;

        let _service_handle = sb.build();

        Ok(DeviceInformationService {})
    }

    fn add_opt_str_characteristic(
        sb: &mut ServiceBuilder,
        uuid: Uuid,
        val: Option<&'static str>,
    ) -> Result<Option<CharacteristicHandles>, RegisterError> {
        if let Some(val) = val {
            let attr = Attribute::new(val);
            let md = Metadata::new(Properties::new().read());
            Ok(Some(sb.add_characteristic(uuid, attr, md)?.build()))
        } else {
            Ok(None)
        }
    }

    fn add_pnp_characteristic(
        sb: &mut ServiceBuilder,
        pnp_id: &PnPID,
    ) -> Result<CharacteristicHandles, RegisterError> {
        // SAFETY: `PnPID` is `repr(C, packed)` so viewing it as an immutable slice of bytes is safe.
        let val = unsafe {
            core::slice::from_raw_parts(
                pnp_id as *const _ as *const u8,
                core::mem::size_of::<PnPID>(),
            )
        };

        let attr = Attribute::new(val);
        let md = Metadata::new(Properties::new().read());
        Ok(sb.add_characteristic(PNP_ID, attr, md)?.build())
    }
}

pub struct BatteryService {
    value_handle: u16,
    cccd_handle: u16,
}

impl BatteryService {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let mut service_builder = ServiceBuilder::new(sd, BATTERY_SERVICE)?;

        let attr = Attribute::new(&[0u8]);
        let metadata =
            Metadata::new(Properties::new().read().notify()).presentation(Presentation {
                format: raw::BLE_GATT_CPF_FORMAT_UINT8 as u8,
                exponent: 0,  /* Value * 10 ^ 0 */
                unit: 0x27AD, /* Percentage */
                name_space: raw::BLE_GATT_CPF_NAMESPACE_BTSIG as u8,
                description: raw::BLE_GATT_CPF_NAMESPACE_DESCRIPTION_UNKNOWN as u16,
            });
        let characteristic_builder =
            service_builder.add_characteristic(BATTERY_LEVEL, attr, metadata)?;
        let characteristic_handles = characteristic_builder.build();

        let _service_handle = service_builder.build();

        Ok(BatteryService {
            value_handle: characteristic_handles.value_handle,
            cccd_handle: characteristic_handles.cccd_handle,
        })
    }

    pub fn battery_level_get(&self, sd: &Softdevice) -> Result<u8, gatt_server::GetValueError> {
        let buf = &mut [0u8];
        gatt_server::get_value(sd, self.value_handle, buf)?;
        Ok(buf[0])
    }

    pub fn battery_level_set(
        &self,
        sd: &Softdevice,
        val: u8,
    ) -> Result<(), gatt_server::SetValueError> {
        gatt_server::set_value(sd, self.value_handle, &[val])
    }
    pub fn battery_level_notify(
        &self,
        conn: &Connection,
        val: u8,
    ) -> Result<(), gatt_server::NotifyValueError> {
        gatt_server::notify_value(conn, self.value_handle, &[val])
    }

    pub fn on_write(&self, handle: u16, data: &[u8]) {
        if handle == self.cccd_handle && !data.is_empty() {
            info!("battery notifications: {}", (data[0] & 0x01) != 0);
        }
    }
}

#[allow(dead_code)]
pub struct HidService {
    hid_info: u16,
    report_map: u16,
    hid_control: u16,
    protocol_mode: u16,
    input_keyboard: u16,
    input_keyboard_cccd: u16,
    input_keyboard_descriptor: u16,
    output_keyboard: u16,
    output_keyboard_descriptor: u16,
}

impl HidService {
    pub fn new(sd: &mut Softdevice) -> Result<Self, RegisterError> {
        let mut service_builder = ServiceBuilder::new(sd, Uuid::new_16(0x1812))?;

        let hid_info = service_builder.add_characteristic(
            HID_INFO,
            Attribute::new([0x11u8, 0x1u8, 0x00u8, 0x01u8]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read()),
        )?;
        let hid_info_handle = hid_info.build();

        let report_map = service_builder.add_characteristic(
            REPORT_MAP,
            Attribute::new(KeyboardReport::desc()).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read()),
        )?;
        let report_map_handle = report_map.build();

        let hid_control = service_builder.add_characteristic(
            HID_CONTROL_POINT,
            Attribute::new([0u8]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().write_without_response()),
        )?;
        let hid_control_handle = hid_control.build();

        let protocol_mode = service_builder.add_characteristic(
            PROTOCOL_MODE,
            Attribute::new([1u8]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read().write_without_response()),
        )?;
        let protocol_mode_handle = protocol_mode.build();

        let mut input_keyboard = service_builder.add_characteristic(
            HID_REPORT,
            Attribute::new([0u8; 8]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read().notify()),
        )?;
        let input_keyboard_desc = input_keyboard.add_descriptor(
            Uuid::new_16(0x2908),
            Attribute::new([1u8, 1u8]).security(SecurityMode::JustWorks),
        )?; // First is ID (e.g. 1 for keyboard 2 for media keys), second is in/out
        let input_keyboard_desc = input_keyboard
            .add_descriptor(Uuid::new_16(0x2908), Attribute::new([KEYBOARD_ID, 1u8]))?; // First is ID (e.g. 1 for keyboard 2 for media keys), second is in/out
        let input_keyboard_handle = input_keyboard.build();

        let mut output_keyboard = service_builder.add_characteristic(
            HID_REPORT,
            Attribute::new([0u8; 8]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read().write().write_without_response()),
        )?;
        let output_keyboard_desc = output_keyboard.add_descriptor(
            Uuid::new_16(0x2908),
            Attribute::new([1u8, 2u8]).security(SecurityMode::JustWorks),
        )?; // First is ID (e.g. 1 for keyboard 2 for media keys)
        let output_keyboard_desc = output_keyboard
            .add_descriptor(Uuid::new_16(0x2908), Attribute::new([KEYBOARD_ID, 2u8]))?; // First is ID (e.g. 1 for keyboard 2 for media keys)
        let output_keyboard_handle = output_keyboard.build();

        let _service_handle = service_builder.build();

        Ok(HidService {
            hid_info: hid_info_handle.value_handle,
            report_map: report_map_handle.value_handle,
            hid_control: hid_control_handle.value_handle,
            protocol_mode: protocol_mode_handle.value_handle,
            input_keyboard: input_keyboard_handle.value_handle,
            input_keyboard_cccd: input_keyboard_handle.cccd_handle,
            input_keyboard_descriptor: input_keyboard_desc.handle(),
            output_keyboard: output_keyboard_handle.value_handle,
            output_keyboard_descriptor: output_keyboard_desc.handle(),
        })
    }

    pub fn on_write(&self, conn: &Connection, handle: u16, data: &[u8]) {
        let val = &[
            0, // Modifiers (Shift, Ctrl, Alt, GUI, etc.)
            0, // Reserved
            0x0E, 0, 0, 0, 0, 0, // Key code array - 0x04 is 'a' and 0x1d is 'z' - for example
        ];
        // gatt_server::notify_value(conn, self.input_keyboard_cccd, val).unwrap();
        // gatt_server::notify_value(conn, self.input_keyboard_descriptor, val).unwrap();
        if handle == self.input_keyboard_cccd {
            info!("HID input keyboard notify: {:?}", data);
        }
    }

    pub fn notify(&self, conn: &Connection, data: &[u8]) {
        match gatt_server::notify_value(&conn, self.input_keyboard, data) {
            Ok(_) => {
                info!("Report Sent!");
            }
            Err(e) => {
                error!("{:?}", e);
            }
        };
    }
}

struct Server {
    _dis: DeviceInformationService,
    bas: BatteryService,
    hid: HidService,
}

impl Server {
    pub fn new(sd: &mut Softdevice, serial_number: &'static str) -> Result<Self, RegisterError> {
        let dis = DeviceInformationService::new(
            sd,
            &PnPID {
                vid_source: VidSource::UsbIF,
                vendor_id: 0xDEAD,
                product_id: 0xBEEF,
                product_version: 0x0000,
            },
            DeviceInformation {
                manufacturer_name: Some("Embassy"),
                model_number: Some("M1234"),
                serial_number: Some(serial_number),
                ..Default::default()
            },
        )?;

        let bas = BatteryService::new(sd)?;

        let hid = HidService::new(sd)?;

        Ok(Self {
            _dis: dis,
            bas,
            hid,
        })
    }
}

impl gatt_server::Server for Server {
    type Event = ();

    fn on_write(
        &self,
        conn: &Connection,
        handle: u16,
        _op: WriteOp,
        _offset: usize,
        data: &[u8],
    ) -> Option<Self::Event> {
        self.hid.on_write(conn, handle, data);
        self.bas.on_write(handle, data);
        None
    }
}

struct HidSecurityHandler {}

impl SecurityHandler for HidSecurityHandler {}

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

    let mut battery_channel = saadc::ChannelConfig::single_ended(p.P0_31);
    battery_channel.gain = Gain::GAIN1;
    let mut saadc = Saadc::new(p.SAADC, Irqs, saadc::Config::default(), [battery_channel]);
    let mut battery = BatteryVoltage::new(&mut saadc, 0).await;

    let channel = SyncChannel::<CriticalSectionRawMutex, u32, 10>::new();
    let tx = channel.sender();
    let rx = channel.receiver();
    let mut matrix = Matrix::new(columns, rows);
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
        peer_config.conn_params.max_conn_interval = 6;
        peer_config.conn_params.slave_latency = 99;
        let peer_conn = central::connect(sd, &peer_config).await.unwrap();

        info!("past the connection");

        let key_client: KeyClient = gatt_client::discover(&peer_conn).await.unwrap();
        info!("past the discover");

        let config = peripheral::Config::default();
        let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };

        let conn = peripheral::advertise_pairable(sd, adv, &config, &SEC)
            .await
            .unwrap();

        conn.set_conn_params(raw::ble_gap_conn_params_t {
            min_conn_interval: 6,
            max_conn_interval: 6,
            slave_latency: 99,
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
                match report.generate_report(&mut keys) {
                    Some(rep) => {
                        let val = [
                            rep.modifier,
                            0,
                            rep.keycodes[0],
                            rep.keycodes[1],
                            rep.keycodes[2],
                            rep.keycodes[3],
                            rep.keycodes[4],
                            rep.keycodes[5],
                        ];
                        server.hid.notify(&conn, &val);
                    }
                    _ => {}
                };
                Timer::after_micros(500).await;
            }
        };

        let res = select4(e, e2, main_loop, battery_loop).await;
        // let res = select(e, main_loop).await;
    }
}
