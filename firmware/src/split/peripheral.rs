use nrf_softdevice::Softdevice;

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

struct Peripheral {
    server: PerpServer,
}

impl Peripheral {
    pub fn init(sd: &mut &mut Softdevice) -> Self {
        let server = PerpServer::new(*sd).unwrap();
        Self { server }
    }

    pub fn connect(&self) {}
}
