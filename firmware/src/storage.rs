use core::{marker::PhantomData, ops::Range};

use defmt::{error, info};
use embassy_nrf::pac::ficr::info;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::Timer;
use embedded_storage_async::nor_flash::{NorFlash, NorFlashError};
use nrf_softdevice::{Flash, FlashError, Softdevice};
use sequential_storage::{
    cache::NoCache,
    erase_all,
    map::{fetch_item, store_item, Key, Value},
};
use static_cell::StaticCell;

use crate::bond::Peer;

pub const NRF_FLASH_RANGE: Range<u32> = (160 * 4096)..(163 * 4096);

pub struct Storage<S: NorFlash, K: Key> {
    flash_range: Range<u32>,
    flash: Mutex<CriticalSectionRawMutex, S>,
    chan: Channel<CriticalSectionRawMutex, (K, StorageItem), 5>,
    _marker: PhantomData<K>,
}

#[derive(Debug, Clone)]
pub enum StorageItem {
    Peer(Peer),
}

impl<S: NorFlash, K: Key> Storage<S, K> {
    /// Returns Storage Struct. This method will clear
    /// the flash range if not intialized.
    pub async fn init(mut flash: S, flash_range: Range<u32>) -> Self {
        info!("Init Stage");
        let mut data_buffer = [0; 128];
        // Check if the key value pair (0x0, 0x69) is in the map
        // If the pair is not in the map, it indicates that the
        // storage isn't initialized
        Timer::after_millis(10).await;
        match fetch_item::<u8, u32, _>(
            &mut flash,
            flash_range.clone(),
            &mut NoCache::new(),
            &mut data_buffer,
            &0x0u8,
        )
        .await
        {
            Ok(res) => match res {
                Some(val) => {
                    if val != 0x69 {
                        erase_all(&mut flash, flash_range.clone()).await;
                        store_item(
                            &mut flash,
                            flash_range.clone(),
                            &mut NoCache::new(),
                            &mut data_buffer,
                            &0x0u8,
                            &0x69u32,
                        )
                        .await;
                        info!("Key Exists, invalid value");
                    } else {
                        info!("Valid Storage");
                    }
                }
                None => {
                    erase_all(&mut flash, flash_range.clone()).await;
                    store_item(
                        &mut flash,
                        flash_range.clone(),
                        &mut NoCache::new(),
                        &mut data_buffer,
                        &0x0u8,
                        &0x69u32,
                    )
                    .await;
                    info!("Key Doesn't exist");
                }
            },
            Err(err) => {
                info!("Error occured");
            }
        };
        Self {
            flash: Mutex::new(flash),
            flash_range,
            chan: Channel::new(),
            _marker: PhantomData,
        }
    }

    pub async fn store_item<'a, V: Value<'a>>(&self, key: K, value: &V) {
        let mut buffer = [0; 128];
        let flash = &mut *(self.flash.lock().await);
        match store_item(
            flash,
            self.flash_range.clone(),
            &mut NoCache::new(),
            &mut buffer,
            &key,
            value,
        )
        .await
        {
            Ok(_) => info!("Item Stored succesfully"),
            Err(_) => error!("Failed to store item"),
        }
    }

    /// Sends item to channel to be stored later. Blocks until message is sent
    pub fn send_item(&self, key: &K, value: &StorageItem) {
        loop {
            info!("Trying to send to channel!");
            match self.chan.try_send((key.clone(), value.clone())) {
                Ok(_) => {
                    info!("Message sent to channel succesfully!");
                    break;
                }
                Err(_) => {
                    info!("Channel is full!");
                }
            }
        }
    }

    /// This method allows non-async methods to write to the storage in a async matter with
    /// channels. Method is not needed if all your functions can be run in async
    pub async fn run_storage(&self) {
        loop {
            let (key, value) = self.chan.receive().await;
            match value {
                StorageItem::Peer(peer) => self.store_item(key, &peer).await,
            };
        }
    }

    pub async fn get_item<'a, V: Value<'a>>(&self, key: K, buffer: &'a mut [u8]) -> Option<V> {
        let flash = &mut *(self.flash.lock().await);
        match fetch_item(
            flash,
            self.flash_range.clone(),
            &mut NoCache::new(),
            buffer,
            &key,
        )
        .await
        {
            Ok(res) => res,
            Err(_) => None,
        }
    }

    pub async fn clear(&self) {
        let flash = &mut *(self.flash.lock().await);
        erase_all(flash, self.flash_range.clone()).await;
    }
}
