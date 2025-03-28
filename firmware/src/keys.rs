use core::{borrow::BorrowMut, ops::Range};

use embassy_time::{Duration, Instant};
use heapless::Vec;

use crate::{codes::KeyCodes, split::dual::DualMode};
pub const NUM_LAYERS: usize = 10;

pub const DEBOUNCE_TIME: u64 = 6;

const CENTRAL_NUM_KEYS: usize = 18;
const PERP_NUM_KEYS: usize = 18;

#[derive(Copy, Clone, Debug)]
struct Position {
    state: bool,
    debounced: Option<Instant>,
}

impl Position {
    const fn default() -> Position {
        Self {
            state: false,
            debounced: None,
        }
    }
    /// Returns the pressed status of the position
    fn is_pressed(&self) -> bool {
        self.state
    }

    fn update_buf(&mut self, buf: bool) {
        self.state = buf;
    }
}

/// Represents a layer scancode. Pos represents the layer
/// the scancode will switch to and toggle will repsent if
/// the layer stored in the code stays after the key is released
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Layer {
    pub pos: usize,
    pub toggle: bool,
}

/// Sends the scan code in intervals which is determined by the passed in delay
/// and passed in equation.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct IntervalPresses {
    code: ScanCode,
    starting_time: Option<Instant>,
    last_pressed_time: Instant,
    org_delay: Duration,
    current_delay: Duration,
    acc_eq: fn(elasped: u64) -> u64,
}
impl IntervalPresses {
    pub fn new(val: ScanCode, delay: Duration, acc_eq: fn(u64) -> u64) -> Self {
        Self {
            code: val,
            starting_time: None,
            last_pressed_time: Instant::now(),
            org_delay: delay,
            current_delay: Duration::default(),
            acc_eq,
        }
    }
    fn get_code(&mut self) -> ScanCode {
        if let Some(time) = self.starting_time {
            if self.last_pressed_time.elapsed() > self.current_delay {
                self.last_pressed_time = Instant::now();
                let val = (self.acc_eq)(time.elapsed().as_millis());
                self.current_delay = Duration::from_millis(self.org_delay.as_micros() / val);
                self.code
            } else {
                ScanCode::None
            }
        } else {
            self.starting_time = Some(Instant::now());
            self.last_pressed_time = Instant::now();
            self.current_delay = self.org_delay;
            self.code
        }
    }
}

/// Represents all the different types of scancodes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ScanCode {
    Letter(u8),
    Modifier(u8),
    MouseButton(u8),
    MouseX(i8),
    MouseY(i8),
    Layer(Layer),
    Scroll(i8),
    None,
}

/// Wrapper around ScanCode to allow different fuctionalites when pressed
/// such as sending multiple keys
#[derive(Copy, Clone, Debug)]
pub enum ScanCodeBehavior<const S: usize> {
    Single(ScanCode),
    Double(ScanCode, ScanCode),
    Triple(ScanCode, ScanCode, ScanCode),
    // Return a different key code depending on the other indexed key press status
    CombinedKey {
        other_index: usize,
        normal_code: ScanCode,
        combined_code: ScanCode,
    },
    IntervalPresses(IntervalPresses),
    Config(fn(&mut Keys<S>)),
    Function(fn()),
}

#[derive(Copy, Clone, Debug)]
struct Key<const S: usize> {
    pos: Position,
    codes: [ScanCodeBehavior<S>; NUM_LAYERS],
    pub current_layer: Option<usize>,
}

impl<const S: usize> Key<S> {
    const fn default() -> Self {
        Self {
            pos: Position::default(),
            codes: [ScanCodeBehavior::Single(ScanCode::Letter(0)); NUM_LAYERS],
            current_layer: None,
        }
    }

    fn set_code(&mut self, code: KeyCodes, toggle: bool, layer: usize) {
        self.codes[layer] = match code.get_scan_code() {
            ScanCode::Layer(mut l) => {
                l.toggle = toggle;
                ScanCodeBehavior::Single(ScanCode::Layer(l))
            }
            rest => ScanCodeBehavior::Single(rest),
        }
    }

    fn update_buf(&mut self, buf: bool) {
        self.pos.update_buf(buf);
    }

    pub fn is_pressed(&self) -> bool {
        self.pos.is_pressed()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Keys<const S: usize> {
    keys: [Key<S>; S],
    state: u32,
}

enum PressResult {
    Pressed,
    Function,
    None,
}
impl<const S: usize> Keys<S> {
    /// Returns a Keys struct
    pub const fn default() -> Self {
        Self {
            keys: [Key::default(); S],
            state: 0,
        }
    }

    pub fn get_pressed(&self, index: usize) -> bool {
        self.keys[index].pos.is_pressed()
    }

    pub fn get_states(&self) -> u32 {
        self.state
    }

    /// Sets the code on the passed in layer on the indexed key. Returns
    /// an err on invalid index or invalid layer
    pub fn set_code(&mut self, code: KeyCodes, index: usize, layer: usize) {
        self.keys[index].set_code(code, false, layer);
    }

    /// Sets the indexed key to be a double key. A double key sends two keycodes rather than one
    pub fn set_double(&mut self, code0: KeyCodes, code1: KeyCodes, index: usize, layer: usize) {
        self.keys[index].codes[layer] =
            ScanCodeBehavior::Double(code0.get_scan_code(), code1.get_scan_code());
    }

    /// Sets the indexed key to be a combined key. other_index is the other indexed key that needs
    /// to be held for the comb_code to be activated
    pub fn set_combined(
        &mut self,
        norm_code: KeyCodes,
        comb_code: KeyCodes,
        other_index: usize,
        index: usize,
        layer: usize,
    ) {
        self.keys[index].codes[layer] = ScanCodeBehavior::CombinedKey {
            other_index,
            normal_code: norm_code.get_scan_code(),
            combined_code: comb_code.get_scan_code(),
        }
    }

    /// Sets the indexed key to be an interval key. An interval key sends a press every dur. The
    /// passed in function represent the rate the dur should change
    pub fn set_interval(
        &mut self,
        code: KeyCodes,
        dur: Duration,
        f: fn(u64) -> u64,
        index: usize,
        layer: usize,
    ) {
        self.keys[index].codes[layer] =
            ScanCodeBehavior::IntervalPresses(IntervalPresses::new(code.get_scan_code(), dur, f))
    }

    /// Sets the following indexed to be a toggle layer key for the passed in layer. Any none layer
    /// keys passed in will be set like in set_code
    pub fn set_toggle_layer(&mut self, layer_code: KeyCodes, index: usize, layer: usize) {
        match layer_code.get_scan_code() {
            ScanCode::Layer(_) => {}
            _ => {
                panic!("bruh")
            }
        }
        self.keys[index].set_code(layer_code, true, layer);
    }

    pub fn set_config(&mut self, f: fn(&mut Keys<S>), index: usize, layer: usize) {
        self.keys[index].codes[layer] = ScanCodeBehavior::Config(f);
    }

    pub fn set_function(&mut self, f: fn(), index: usize, layer: usize) {
        self.keys[index].codes[layer] = ScanCodeBehavior::Function(f);
    }

    /// Updates the indexed key with the provided reading
    pub fn update_buf(&mut self, index: usize, buf: bool) {
        self.keys[index].update_buf(buf);
        if index < CENTRAL_NUM_KEYS {
            if buf {
                self.state |= 1 << index;
            } else {
                self.state &= !(1 << index);
            }
        }
    }

    /// Updates the indexed key with the provided reading
    pub fn update_buf_central(&mut self, index: usize, buf: bool) {
        if index < CENTRAL_NUM_KEYS {
            self.update_buf(index, buf);
        }
    }

    /// Returns the indexes of all the keys that are pressed to the vec
    pub fn is_pressed(&self, vec: &mut Vec<usize, S>) {
        for i in 0..S {
            if self.keys[i].pos.is_pressed() {
                vec.push(i).unwrap();
            }
        }
    }

    /// Pushes the resulting ScanResult onto the provided vec depending on the indexed key's
    /// position. Returns true if a key was pushed into the provided index set
    fn get_pressed_code(
        &mut self,
        index: usize,
        layer: usize,
        set: &mut Vec<ScanCode, 64>,
    ) -> PressResult {
        let pressed = self.keys[index].pos.is_pressed();
        match self.keys[index].codes[layer].borrow_mut() {
            ScanCodeBehavior::Single(code) => {
                if pressed {
                    set.push(*code).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::Double(code0, code1) => {
                if pressed {
                    set.push(*code0).unwrap();
                    set.push(*code1).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::Triple(code0, code1, code2) => {
                if pressed {
                    set.push(*code0).unwrap();
                    set.push(*code1).unwrap();
                    set.push(*code2).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::CombinedKey {
                other_index,
                normal_code,
                combined_code: other_key_code,
            } => {
                if pressed {
                    if self.keys[*other_index].pos.is_pressed() {
                        set.push(*other_key_code).unwrap();
                        PressResult::Pressed
                    } else {
                        set.push(*normal_code).unwrap();
                        PressResult::Pressed
                    }
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::IntervalPresses(val) => {
                if pressed {
                    set.push(val.get_code()).unwrap();
                    PressResult::Pressed
                } else {
                    val.starting_time = None;
                    PressResult::None
                }
            }
            ScanCodeBehavior::Config(f) => {
                if pressed {
                    f(self);
                    PressResult::Function
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::Function(f) => {
                if pressed {
                    f();
                    PressResult::Function
                } else {
                    PressResult::None
                }
            }
        }
    }

    /// Returns all the pressed scancodes in the Keys struct. Returns it through
    /// the passed in vector. This function won't return layer codes. That will be done
    /// through the get_layer method. The passed in vector should be empty.
    /// Note that if a key is held, it will ignore the passed in layer and use the
    /// previous layer it's holding
    pub fn get_keys(&mut self, layer: usize, set: &mut Vec<ScanCode, 64>) {
        for i in 0..S {
            let layer = match self.keys[i].current_layer {
                Some(num) => num,
                None => layer,
            };
            match self.get_pressed_code(i, layer, set) {
                PressResult::Function => {
                    set.clear();
                    break;
                }
                PressResult::Pressed => {
                    self.keys[i].current_layer = Some(layer);
                }
                PressResult::None => {
                    self.keys[i].current_layer = None;
                }
            }
        }
    }
}
