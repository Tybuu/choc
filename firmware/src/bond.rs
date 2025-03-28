use core::{
    cell::RefCell,
    mem,
    sync::atomic::{AtomicU8, Ordering},
};

use defmt::{error, info};
use embassy_time::Instant;
use embedded_storage_async::nor_flash::NorFlash;
use heapless::{FnvIndexMap, Vec};
use nrf_softdevice::ble::{
    gatt_server, security::SecurityHandler, Address, Connection, EncryptionInfo, IdentityKey,
    IdentityResolutionKey, MasterId,
};
use sequential_storage::map::Value;

use crate::storage::{Storage, StorageItem};

const PEER_SIZE: usize = mem::size_of::<Peer>();
const MAX_NUM_BONDS: usize = 8;
const BOND_START: u32 = 1;

pub const fn bi(i: u8) -> u32 {
    BOND_START + i as u32
}
#[repr(u8)]
enum IndexState {
    Zero = 0,
    One = 1,
}

fn get_indices(state: u8, vec: &mut Vec<u8, 8>, index_state: IndexState) {
    let num = index_state as u8;
    for i in 0..8 {
        if (state & (1u8 << i)) >> i == num {
            vec.push(i).unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    index: u8,
    master_id: MasterId,
    key: EncryptionInfo,
    peer_id: IdentityKey,
    sys_attrs: Vec<u8, 62>,
}

impl Peer {
    pub fn new() -> Self {
        Self {
            index: 0u8,
            master_id: MasterId {
                ediv: 0u16,
                rand: [0u8; 8],
            },
            key: EncryptionInfo {
                ltk: [0u8; 16],
                flags: 0u8,
            },
            peer_id: IdentityKey {
                irk: IdentityResolutionKey::from_raw(nrf_softdevice::raw::ble_gap_irk_t {
                    irk: [0u8; 16],
                }),
                addr: Address {
                    flags: 0u8,
                    bytes: [0u8; 6],
                },
            },
            sys_attrs: Vec::new(),
        }
    }
}

impl<'a> Value<'a> for Peer {
    fn serialize_into(
        &self,
        buffer: &mut [u8],
    ) -> Result<usize, sequential_storage::map::SerializationError> {
        if buffer.len() < PEER_SIZE {
            return Err(sequential_storage::map::SerializationError::BufferTooSmall);
        }
        // Master Id
        let mut i = 0;
        buffer[i..(i + 2)].copy_from_slice(&self.master_id.ediv.to_le_bytes());
        i += 2;

        buffer[i..(i + self.master_id.rand.len())].copy_from_slice(&self.master_id.rand);
        i += self.master_id.rand.len();

        // Key
        buffer[i..(i + self.key.ltk.len())].copy_from_slice(&self.key.ltk);
        i += self.key.ltk.len();

        buffer[i] = self.key.flags;
        i += 1;

        // Peer ID
        buffer[i..i + self.peer_id.irk.as_raw().irk.len()]
            .copy_from_slice(&self.peer_id.irk.as_raw().irk);
        i += self.peer_id.irk.as_raw().irk.len();

        buffer[i] = self.peer_id.addr.flags;
        i += 1;

        buffer[i..i + self.peer_id.addr.bytes().len()].copy_from_slice(&self.peer_id.addr.bytes());
        i += self.peer_id.addr.bytes().len();

        // sys_attrs
        // won't overflow as max len is 62
        buffer[i] = self.sys_attrs.len() as u8;
        i += 1;

        buffer[i..i + self.sys_attrs.len()].copy_from_slice(&self.sys_attrs);
        i += self.sys_attrs.len();

        return Ok(i);
    }

    fn deserialize_from(
        buffer: &'a [u8],
    ) -> Result<Self, sequential_storage::map::SerializationError>
    where
        Self: Sized,
    {
        let master_id = MasterId {
            ediv: u16::from_le_bytes(buffer[0..2].try_into().unwrap()),
            rand: buffer[2..10].try_into().unwrap(),
        };

        let key = EncryptionInfo {
            ltk: buffer[10..26].try_into().unwrap(),
            flags: buffer[26],
        };

        let peer_id = IdentityKey {
            irk: IdentityResolutionKey::from_raw(nrf_softdevice::raw::ble_gap_irk_t {
                irk: buffer[27..43].try_into().unwrap(),
            }),
            addr: Address {
                flags: buffer[43],
                bytes: buffer[44..50].try_into().unwrap(),
            },
        };

        let vec_size = buffer[50];
        Ok(Self {
            index: 0,
            master_id,
            key,
            peer_id,
            sys_attrs: {
                let mut vec = Vec::<u8, 62>::new();
                vec.extend_from_slice(&buffer[51..(51 + vec_size as usize)])
                    .unwrap();
                vec
            },
        })
    }
}
pub struct Bonder<'a, S: NorFlash> {
    bonds: RefCell<FnvIndexMap<u8, Peer, MAX_NUM_BONDS>>,
    storage: &'a Storage<S, u32>,
}

impl<'a, S: NorFlash> Bonder<'a, S> {
    pub async fn init(storage: &'a Storage<S, u32>) -> Self {
        let bonder = Bonder {
            bonds: RefCell::new(FnvIndexMap::new()),
            storage,
        };

        let mut buffer = [0u8; 128];
        let mut bonds = bonder.bonds.borrow_mut();

        for i in 0..(MAX_NUM_BONDS as u8) {
            storage
                .get_item::<Peer>(bi(i), &mut buffer)
                .await
                .map(|mut peer| {
                    info!("Mapped one peer");
                    peer.index = i;
                    bonds.insert(i, peer).unwrap();
                });
        }
        drop(bonds);
        bonder
    }
}

impl<'a, S: NorFlash> SecurityHandler for Bonder<'a, S> {
    fn io_capabilities(&self) -> nrf_softdevice::ble::security::IoCapabilities {
        nrf_softdevice::ble::security::IoCapabilities::None
    }

    fn can_bond(&self, conn: &Connection) -> bool {
        true
    }

    fn on_bonded(
        &self,
        _conn: &Connection,
        master_id: MasterId,
        key: EncryptionInfo,
        peer_id: IdentityKey,
    ) {
        info!("On bonded");
        let mut bonds = self.bonds.borrow_mut();

        // Checks if the bond is already in the map and replaces the bond
        // if it exists, otherwise find an empty index or randomly replace a bond
        // if all slots are used
        let index = if let Some((i, _)) = bonds.iter().find(|(_, p)| p.master_id == master_id) {
            *i as u8
        } else {
            (Instant::now().as_ticks() % MAX_NUM_BONDS as u64) as u8
        };
        let val = Peer {
            index,
            master_id,
            key,
            peer_id,
            sys_attrs: Vec::new(),
        };

        bonds.insert(index, val.clone()).unwrap();
        self.storage.send_item(&bi(index), &StorageItem::Peer(val));
    }

    fn get_key(&self, conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        info!("Get Key");

        info!("Passed Master ID {:?}", master_id);
        let bonds = self.bonds.borrow();
        let key = bonds.iter().find_map(|(key, peer)| {
            info!("Storage Master ID {:?}", peer.master_id);
            if peer.master_id == master_id {
                info!("Found key!");
                Some(peer.key)
            } else {
                None
            }
        });
        if key.is_none() {
            info!("No key found!")
        }
        self.load_sys_attrs(conn);
        key
    }

    fn save_sys_attrs(&self, conn: &Connection) {
        info!("Save Sys Attrs");
        let mut buffer = [0u8; 128];
        let mut bonds = self.bonds.borrow_mut();
        let res = bonds.iter_mut().find_map(|(key, peer)| {
            if peer.peer_id.is_match(conn.peer_address()) {
                info!("Found Peer!");
                match gatt_server::get_sys_attrs(conn, &mut buffer) {
                    Ok(len) => {
                        if !(*peer.sys_attrs.as_slice() == buffer[..len]) {
                            info!("Saving sys_attrs");
                            peer.sys_attrs.clear();
                            peer.sys_attrs.extend_from_slice(&buffer[..len]).unwrap();
                            self.storage
                                .send_item(&bi(*key), &StorageItem::Peer(peer.clone()));
                        }
                    }
                    Err(err) => {
                        error!("GATT gys_attrs get error: {}", err);
                    }
                }
                Some(())
            } else {
                None
            }
        });
        if res.is_none() {
            info!("Found no peer");
        }
    }

    fn load_sys_attrs(&self, conn: &Connection) {
        info!("Load Sys Attrs");
        let bonds = self.bonds.borrow();
        let res = bonds.iter().find_map(|(key, peer)| {
            if peer.peer_id.is_match(conn.peer_address()) {
                let attrs = if peer.sys_attrs.is_empty() {
                    None
                } else {
                    Some(peer.sys_attrs.as_slice())
                };
                gatt_server::set_sys_attrs(conn, attrs);
                Some(())
            } else {
                None
            }
        });
        if res.is_none() {
            info!("Found no peer")
        }
    }
}
