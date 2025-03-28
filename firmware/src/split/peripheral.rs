use core::borrow::Borrow;

use embassy_futures::select::select;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use nrf_softdevice::{
    ble::{
        advertisement_builder::{
            Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload, ServiceList,
        },
        gatt_server, peripheral,
    },
    Softdevice,
};

#[nrf_softdevice::gatt_service(uuid = "9e7312e0-2354-11eb-9f10-fbc30a62cf38")]
struct KeyClient {
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf38", read, write, notify)]
    state: u32,
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf39", read, write, notify)]
    mouse_state: u16,
}

#[nrf_softdevice::gatt_server]
struct PerpServer {
    key_client: KeyClient,
}

pub struct BlePeripheral {
    server: PerpServer,
    channel: Channel<CriticalSectionRawMutex, u32, 20>,
    status: Mutex<CriticalSectionRawMutex, bool>,
}

impl BlePeripheral {
    pub fn init(sd: &mut Softdevice) -> Self {
        let server = PerpServer::new(sd).unwrap();
        Self {
            server,
            channel: Channel::new(),
            status: Mutex::new(false),
        }
    }

    pub async fn clear(&self) {
        self.channel.clear();
        let mut status = self.status.lock().await;
        *status = false;
    }

    pub async fn connect(&self) {
        static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
            .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
            .services_128(
                ServiceList::Complete,
                &[0x9e7312e0_2354_11eb_9f10_fbc30a62cf38_u128.to_le_bytes()],
            )
            .full_name("TybeastR")
            .build();

        let config = peripheral::Config::default();
        static SCAN_DATA: [u8; 0] = [];
        let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };
        let sd = unsafe { Softdevice::steal() };
        let mut conn = peripheral::advertise_connectable(sd, adv, &config)
            .await
            .unwrap();

        let channel_handler = async {
            loop {
                let rep = self.channel.receiver().receive().await;
                self.server.key_client.state_notify(&conn, &rep);
            }
        };
        let e = gatt_server::run(&conn, &self.server, |_| {});

        {
            let mut status = self.status.lock().await;
            *status = true;
        }

        select(e, channel_handler).await;

        {
            let mut status = self.status.lock().await;
            *status = false;
        }
    }

    pub async fn state_notify(&self, rep: u32) {
        let active = self.status.lock().await;
        if *active {
            self.channel.sender().send(rep).await;
        }
    }
}
