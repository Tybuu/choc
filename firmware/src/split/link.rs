use core::{cell::UnsafeCell, future::Future, mem::MaybeUninit};

use embassy_sync::{blocking_mutex::raw::RawMutex, channel::Sender};
use nrf_softdevice::{
    ble::{
        central,
        gatt_client::{self, Client},
        Address, Connection,
    },
    Softdevice,
};

#[nrf_softdevice::gatt_client(uuid = "9e7312e0-2354-11eb-9f10-fbc30a62cf38")]
struct KeyClient {
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf38", read, write, notify)]
    state: u32,
    #[characteristic(uuid = "9e7312e0-2354-11eb-9f10-fbc30a63cf39", read, write, notify)]
    mouse_state: u16,
}

pub struct Link<'a, M: RawMutex, const N: usize> {
    tx: Sender<'a, M, u32, N>,
}

impl<'a, M: RawMutex, const N: usize> Link<'a, M, N> {
    pub fn new(tx: Sender<'a, M, u32, N>) -> Self {
        Self { tx }
    }

    pub async fn link<'b>(&'b mut self, addr: Address) {
        let peer_addr = [&addr];
        let mut peer_config = central::ConnectConfig::default();
        peer_config.scan_config.whitelist = Some(&peer_addr);
        peer_config.conn_params.min_conn_interval = 6;
        peer_config.conn_params.max_conn_interval = 6;
        peer_config.conn_params.slave_latency = 99;

        // Safe as only mut sd ref was when creating servers
        let sd = unsafe { Softdevice::steal() };

        let peer_conn = central::connect(sd, &peer_config).await.unwrap();
        let key_client: KeyClient = gatt_client::discover(&peer_conn).await.unwrap();

        key_client.state_cccd_write(true).await.unwrap();
        key_client.mouse_state_cccd_write(true).await.unwrap();
        let e2 = gatt_client::run(&peer_conn, &key_client, |event| match event {
            KeyClientEvent::StateNotification(val) => match self.tx.try_send(val) {
                Ok(_) => {}
                Err(_) => {}
            },
            KeyClientEvent::MouseStateNotification(val) => {
                let x = ((val & 0xFF00) >> 8) as u8;
                let y = (val & 0xFF) as u8;

                let buf = [0u8, x, y, 0, 0];
            }
        });
        e2.await;
    }
}
