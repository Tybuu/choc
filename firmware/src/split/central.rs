use core::{
    borrow::Borrow,
    cell::{Ref, RefCell},
    ops::Deref,
};

use defmt::{error, info};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal, watch::Watch,
};
use embedded_storage_async::nor_flash::NorFlash;
use nrf_softdevice::{
    ble::{
        advertisement_builder::{
            AdvertisementDataType, Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload,
            ServiceList, ServiceUuid16,
        },
        gatt_server::{
            self,
            builder::ServiceBuilder,
            characteristic::{Attribute, Metadata, Presentation, Properties},
            CharacteristicHandles, RegisterError, WriteOp,
        },
        peripheral,
        security::SecurityHandler,
        Connection, SecurityMode, Uuid,
    },
    raw, Softdevice,
};
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};

use crate::{
    bond::Bonder,
    descriptor::{CombinedReport, MouseReport},
};

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

const KEYBOARD_ID: u8 = 0x01;

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
    input_mouse: u16,
    input_mouse_cccd: u16,
    intput_mouse_descriptor: u16,
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
            Attribute::new(CombinedReport::desc()).security(SecurityMode::JustWorks),
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

        let mut input_mouse = service_builder.add_characteristic(
            HID_REPORT,
            Attribute::new([0u8; 5]).security(SecurityMode::JustWorks),
            Metadata::new(Properties::new().read().notify()),
        )?;

        let input_mouse_desc = input_mouse.add_descriptor(
            Uuid::new_16(0x2908),
            Attribute::new([2, 1u8]).security(SecurityMode::JustWorks),
        )?;

        let input_mouse_handle = input_mouse.build();

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
            input_mouse: input_mouse_handle.value_handle,
            input_mouse_cccd: input_mouse_handle.cccd_handle,
            intput_mouse_descriptor: input_mouse_desc.handle(),
        })
    }

    pub fn on_write(&self, conn: &Connection, handle: u16, data: &[u8]) {
        // gatt_server::notify_value(conn, self.input_keyboard_cccd, val).unwrap();
        // gatt_server::notify_value(conn, self.input_keyboard_descriptor, val).unwrap();
        if handle == self.input_keyboard_cccd {
            info!("HID input keyboard notify: {:?}", data);
        }
    }

    pub fn mouse_notify(&self, conn: &Connection, data: &[u8]) {
        match gatt_server::notify_value(&conn, self.input_mouse, data) {
            Ok(_) => {
                info!("Report Sent!");
            }
            Err(e) => {
                error!("{:?}", e);
            }
        }
    }
    pub fn keyboard_notify(&self, conn: &Connection, data: &[u8]) {
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

pub struct Server {
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

pub struct BleCentral {
    server: Server,
    conn: RefCell<Option<Connection>>,
    status: Mutex<CriticalSectionRawMutex, bool>,
}

impl BleCentral {
    pub fn init(sd: &mut Softdevice) -> Self {
        let server = Server::new(sd, "12345678").unwrap();
        Self {
            server,
            conn: RefCell::new(None),
            status: Mutex::new(false),
        }
    }

    pub async fn advertise_and_connect<S: NorFlash>(&self, bonder: &'static Bonder<'_, S>) {
        static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
            .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
            .services_16(
                ServiceList::Incomplete,
                &[
                    ServiceUuid16::BATTERY,
                    ServiceUuid16::HUMAN_INTERFACE_DEVICE,
                ],
            )
            .full_name("TybeastL")
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

        let config = peripheral::Config::default();
        let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };

        // Safe as only mut sd ref existed only when creating servers
        let sd = unsafe { Softdevice::steal() };
        let conn = peripheral::advertise_connectable(sd, adv, &config)
            .await
            .unwrap();

        {
            let mut con_status = self.conn.borrow_mut();
            let conn = con_status.insert(conn);
            conn.set_conn_params(raw::ble_gap_conn_params_t {
                min_conn_interval: 6,
                max_conn_interval: 6,
                slave_latency: 99,
                conn_sup_timeout: 400,
            })
            .unwrap();
            let mut status = self.status.lock().await;
            *status = true;
        }

        gatt_server::run(self.conn.borrow().as_ref().unwrap(), &self.server, |_| {}).await;
        {
            self.conn.replace(None);
            let mut status = self.status.lock().await;
            *status = false;
        }
    }

    pub async fn keyboard_notify(&self, rep: &KeyboardReport) {
        let active = self.status.lock().await;
        if *active {
            if let Some(conn) = self.conn.borrow().clone() {
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
                self.server.hid.keyboard_notify(&conn, &val);
            }
        }
    }

    pub async fn mouse_notify(&self, rep: &MouseReport) {
        let active = self.status.lock().await;
        if *active {
            if let Some(conn) = self.conn.borrow().clone() {
                let buf = [
                    rep.buttons,
                    rep.x as u8,
                    rep.y as u8,
                    rep.wheel as u8,
                    rep.pan as u8,
                ];
                self.server.hid.mouse_notify(&conn, &buf);
            }
        }
    }

    pub async fn battery_notify(&self, percentage: u8) {
        let active = self.status.lock().await;
        if *active {
            if let Some(conn) = self.conn.borrow().clone() {
                self.server.bas.battery_level_notify(&conn, percentage);
            }
        }
    }
}
