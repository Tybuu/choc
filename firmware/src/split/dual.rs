use core::mem::MaybeUninit;

use embassy_futures::join::join;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use nrf_softdevice::Softdevice;
use static_cell::StaticCell;

use crate::keys::Keys;
use crate::report::Report;

use super::central::BleCentral;
use super::peripheral::BlePeripheral;

#[derive(Copy, Clone, Debug)]
pub enum DualMode {
    Central,
    Peripheral,
}
pub struct Dual {
    central: BleCentral,
    perp: BlePeripheral,
    mode: Mutex<CriticalSectionRawMutex, DualMode>,
    channel: Channel<CriticalSectionRawMutex, DualMode, 5>,
    state: u32,
}

impl Dual {
    pub fn new(sd: &mut Softdevice) -> Dual {
        let central = BleCentral::init(sd);
        let perp = BlePeripheral::init(sd);
        Dual {
            central,
            perp,
            mode: Mutex::new(DualMode::Peripheral),
            channel: Channel::new(),
            state: 0,
        }
    }

    pub async fn connect(&self) {
        loop {
            let mode_ref = self.mode.lock().await;
            let mode = *mode_ref;
            drop(mode_ref);
            let new_mode = match mode {
                DualMode::Central => {
                    match select(self.central.connect(), self.channel.receiver().receive()).await {
                        embassy_futures::select::Either::First(_) => DualMode::Central,
                        embassy_futures::select::Either::Second(val) => val,
                    }
                }
                DualMode::Peripheral => {
                    match select(self.central.connect(), self.channel.receiver().receive()).await {
                        embassy_futures::select::Either::First(_) => DualMode::Peripheral,
                        embassy_futures::select::Either::Second(val) => val,
                    }
                }
            };
            join(self.central.clear(), self.perp.clear()).await;
            let mut mode_ref = self.mode.lock().await;
            *mode_ref = new_mode;
        }
    }

    pub async fn report<const N: usize>(&self, keys: &mut Keys<N>, report: &mut Report) {
        let mode_ref = self.mode.lock().await;
        let mode = *mode_ref;
        drop(mode_ref);
        match mode {
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
                let state = keys.get_states();
                if self.state != state {
                    self.perp.state_notify(state).await;
                }
            }
        }
    }

    pub async fn battery_notify(&self, percentage: u8) {
        let mode_ref = self.mode.lock().await;
        let mode = *mode_ref;
        drop(mode_ref);
        match mode {
            DualMode::Central => {
                self.central.battery_notify(percentage).await;
            }
            DualMode::Peripheral => {}
        }
    }
}
