use embassy_time::{Duration, Instant};
use heapless::{FnvIndexSet, Vec};
use usbd_hid::descriptor::KeyboardReport;

use crate::{
    descriptor::{KeyboardReportNKRO, MouseReport},
    keys::{Keys, ScanCode},
};

fn set_bit(num: &mut u8, bit: u8, pos: u8) {
    let mask = 1 << pos;
    if bit == 1 {
        *num |= mask
    } else {
        *num &= !mask
    }
}

pub struct Report {
    key_report: KeyboardReport,
    mouse_report: MouseReport,
    last_report_time: Instant,
    current_layer: usize,
    reset_layer: usize,
    key_states: u32,
}
impl Report {
    pub fn default() -> Self {
        Self {
            key_report: KeyboardReport::default(),
            mouse_report: MouseReport::default(),
            last_report_time: Instant::now(),
            current_layer: 0,
            reset_layer: 0,
            key_states: 0,
        }
    }

    /// Generates a report with the provided keys. Returns a option tuple
    /// where it returns a Some when a report need to be sent
    pub fn generate_report<const S: usize>(
        &mut self,
        keys: &mut Keys<S>,
    ) -> (Option<&KeyboardReport>, Option<&MouseReport>) {
        let mut new_layer = None;
        let mut pressed_keys = Vec::<ScanCode, 64>::new();
        let mut new_key_report = KeyboardReport::default();
        let mut new_mouse_report = MouseReport::default();

        keys.get_keys(self.current_layer, &mut pressed_keys);
        let mut index = 0;
        for key in &pressed_keys {
            match key {
                ScanCode::Modifier(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_key_report.modifier, 1, b_idx);
                }
                ScanCode::Letter(code) => {
                    // let n_idx = (code / 8) as usize;
                    // let b_idx = code % 8;
                    // set_bit(&mut new_key_report.nkro_keycodes[n_idx], 1, b_idx);
                    if index < 6 {
                        new_key_report.keycodes[index] = *code;
                        index += 1;
                    }
                }
                ScanCode::MouseButton(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_mouse_report.buttons, 1, b_idx);
                }
                ScanCode::MouseX(code) => {
                    new_mouse_report.x += code * 10;
                }
                ScanCode::MouseY(code) => {
                    new_mouse_report.y += code * 10;
                }
                ScanCode::Scroll(code) => {
                    new_mouse_report.wheel += code;
                }
                ScanCode::Layer(layer) => match new_layer {
                    Some(_) => {
                        if layer.toggle {
                            new_layer = Some(layer);
                        }
                    }
                    None => {
                        new_layer = Some(layer);
                    }
                },
                ScanCode::None => {}
            };
        }
        match new_layer {
            Some(layer) => {
                if layer.toggle {
                    self.reset_layer = layer.pos;
                }
                self.current_layer = layer.pos;
            }
            None => {
                self.current_layer = self.reset_layer;
            }
        }
        let mut key_report = None;
        let mut mouse_report = None;
        if self.key_report.keycodes != new_key_report.keycodes
            || self.key_report.modifier != new_key_report.modifier
        {
            self.key_report = new_key_report;
            key_report = Some(&self.key_report)
        }
        if (self.mouse_report.buttons != new_mouse_report.buttons
            || new_mouse_report.x != 0
            || new_mouse_report.y != 0
            || new_mouse_report.wheel != 0)
            && self.last_report_time.elapsed() >= Duration::from_millis(20)
        {
            self.last_report_time = Instant::now();
            self.mouse_report = new_mouse_report;
            mouse_report = Some(&self.mouse_report);
        }
        (key_report, mouse_report)
    }

    pub fn generate_state<const S: usize>(&mut self, keys: &mut Keys<S>) -> bool {
        let mut pressed_keys = Vec::<ScanCode, 64>::new();
        let mut new_layer = None;
        keys.get_keys(self.current_layer, &mut pressed_keys);
        for key in &pressed_keys {
            match key {
                ScanCode::Layer(layer) => match new_layer {
                    Some(_) => {
                        if layer.toggle {
                            new_layer = Some(layer);
                        }
                    }
                    None => {
                        new_layer = Some(layer);
                    }
                },
                _ => {}
            };
        }
        match new_layer {
            Some(layer) => {
                if layer.toggle {
                    self.reset_layer = layer.pos;
                }
                self.current_layer = layer.pos;
            }
            None => {
                self.current_layer = self.reset_layer;
            }
        }
        if self.key_states != keys.get_states() {
            self.key_states = keys.get_states();
            return true;
        } else {
            return false;
        }
    }
}
