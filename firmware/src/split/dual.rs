use core::mem::MaybeUninit;

use embassy_futures::join::join;
use embassy_futures::select::{select, select3};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, RawMutex};
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use nrf_softdevice::ble::Address;
use nrf_softdevice::Softdevice;

use crate::keys::Keys;
use crate::report::Report;

use super::central::BleCentral;
use super::link::Link;
use super::peripheral::BlePeripheral;

#[derive(Copy, Clone, Debug)]
pub enum DualMode {
    Central,
    Peripheral,
}

static DUAL_NAME: &str = "TybeastL";

pub struct Dual {
    central: BleCentral,
    perp: BlePeripheral,
    mode: DualMode,
}

impl Dual {
    pub fn new(sd: &mut Softdevice, mode: DualMode) -> Dual {
        let central = BleCentral::init(sd);
        let perp = BlePeripheral::init(sd);
        Dual {
            central,
            perp,
            mode,
        }
    }

    pub async fn connect<'a, M: RawMutex, const N: usize>(
        &self,
        link: &Link<'a, M, N>,
        pair_addr: Address,
    ) {
        match self.mode {
            DualMode::Central => {
                select(self.central.connect(), link.link(pair_addr)).await;
                self.central.clear().await;
            }
            DualMode::Peripheral => {
                self.perp.connect(DUAL_NAME).await;
                self.perp.clear().await;
            }
        };
    }

    pub async fn report<const N: usize>(&self, keys: &mut Keys<N>, report: &mut Report) {
        match self.mode {
            DualMode::Central => {
                let (key, mouse) = report.generate_report(keys);
                match key {
                    Some(rep) => {
                        self.central.keyboard_notify(rep).await;
                    }
                    _ => {}
                };
                match mouse {
                    Some(rep) => {
                        self.central.mouse_notify(rep).await;
                    }
                    None => {}
                }
            }
            DualMode::Peripheral => {
                if report.generate_state(keys) {
                    self.perp.state_notify(keys.get_states()).await
                }
            }
        }
    }

    pub async fn battery_notify(&self, percentage: u8) {
        match self.mode {
            DualMode::Central => {
                self.central.battery_notify(percentage).await;
            }
            DualMode::Peripheral => {}
        }
    }
}
