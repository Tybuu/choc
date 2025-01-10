use embassy_time::Instant;
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
    current_layer: usize,
    reset_layer: usize,
}

impl Report {
    pub fn default() -> Self {
        Self {
            key_report: KeyboardReport::default(),
            mouse_report: MouseReport::default(),
            current_layer: 0,
            reset_layer: 0,
        }
    }

    /// Generates a report with the provided keys. Returns a option tuple
    /// where it returns a Some when a report need to be sent
    pub fn generate_report<const S: usize>(
        &mut self,
        keys: &mut Keys<S>,
    ) -> Option<&KeyboardReport> {
        let mut new_layer = None;
        let mut pressed_keys = Vec::<ScanCode, 64>::new();
        let mut new_key_report = KeyboardReport::default();
        let mut new_mouse_report = MouseReport::default();
        let mut index = 0;
        keys.get_keys(self.current_layer, &mut pressed_keys);
        for key in &pressed_keys {
            match key {
                ScanCode::Modifier(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_key_report.modifier, 1, b_idx);
                }
                ScanCode::Letter(code) => {
                    let n_idx = (code / 8) as usize;
                    let b_idx = code % 8;
                    if index < 6 {
                        new_key_report.keycodes[index as usize] = *code as u8;
                        index += 1;
                    }
                }
                ScanCode::MouseButton(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_mouse_report.buttons, 1, b_idx);
                }
                ScanCode::MouseX(code) => {
                    new_mouse_report.x += code;
                }
                ScanCode::MouseY(code) => {
                    new_mouse_report.y += code;
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
        if self.key_report.keycodes != new_key_report.keycodes
            || self.key_report.modifier != new_key_report.modifier
        {
            self.key_report = new_key_report;
            Some(&self.key_report)
        } else {
            None
        }
        // let mut returned_report = (None, None);
        // Second bool condtion is needed as the mouse report is relative.
        // If a key is held, we need to constantly send reports to represent
        // that state to the host
        // if self.mouse_report != new_mouse_report || new_mouse_report != MouseReport::default() {
        //     self.mouse_report = new_mouse_report;
        //     returned_report.1 = Some(&self.mouse_report);
        // }
    }
}
