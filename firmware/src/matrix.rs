use embassy_futures::select::{select4, select_array, select_slice};
use embassy_nrf::{
    gpio::{AnyPin, Input, Output},
    gpiote::{AnyChannel, InputChannel},
};
use embassy_time::{Duration, Instant};
use embedded_hal_async::digital::Wait;
use heapless::Vec;
use rand::seq::IteratorRandom;

const DEBOUNCE_TIME: u64 = 5;
#[derive(Copy, Clone, Debug)]
struct Debouncer {
    state: bool,
    debounced: Option<Instant>,
}

impl Debouncer {
    const fn default() -> Debouncer {
        Self {
            state: false,
            debounced: None,
        }
    }
    /// Returns the pressed status of the position
    fn is_pressed(&self) -> bool {
        self.state
    }

    /// Updates the buf of the key. Updating the buf will also update
    /// the value returned from the is_pressed function
    fn update_buf(&mut self, buf: bool) {
        match self.debounced {
            Some(time) => {
                if time.elapsed() > Duration::from_millis(DEBOUNCE_TIME) {
                    self.debounced = None;
                }
            }
            None => {
                if buf != self.state {
                    self.debounced = Some(Instant::now());
                    self.state = buf;
                }
            }
        }
    }
}
pub struct Matrix<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> {
    out: [Output<'a, AnyPin>; OUTPUT_SIZE],
    input: [Input<'a, AnyPin>; INPUT_SIZE],
    debouncers: [[Debouncer; OUTPUT_SIZE]; INPUT_SIZE],
    pressed: Option<Instant>,
}

impl<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> Matrix<'a, INPUT_SIZE, OUTPUT_SIZE> {
    pub fn new(
        out: [Output<'a, AnyPin>; OUTPUT_SIZE],
        input: [Input<'a, AnyPin>; INPUT_SIZE],
    ) -> Self {
        Self {
            out,
            input,
            debouncers: [[Debouncer::default(); OUTPUT_SIZE]; INPUT_SIZE],
            pressed: None,
        }
    }

    // Only returns if a key is high or a previous scan had a key that was high. Otherwise,
    // awaits for a high key
    pub async fn scan(&mut self, states: &mut [[bool; OUTPUT_SIZE]; INPUT_SIZE]) {
        // If no keys were pressed in the previous scan,
        // we'll set all the output pins high and await
        // for one of the channels to go high to save battery
        if let Some(time) = self.pressed {
            if time.elapsed() >= Duration::from_millis(DEBOUNCE_TIME) {
                for power in &mut self.out {
                    power.set_high();
                }

                // let mut high = false;
                // for row in &mut self.input {
                //     high = high || row.is_high()
                // }

                let futures: Vec<_, INPUT_SIZE> = self
                    .input
                    .iter_mut()
                    .map(|pin| pin.wait_for_high())
                    .collect();
                unsafe {
                    select_array(futures.into_array::<INPUT_SIZE>().unwrap_unchecked()).await;
                }

                for power in &mut self.out {
                    power.set_low();
                }
            }
        }

        let mut pressed = false;
        for i in 0..OUTPUT_SIZE {
            self.out[i].set_high();
            for j in 0..INPUT_SIZE {
                states[j][i] = self.input[j].is_high();
                pressed = pressed || states[j][i];
            }
            self.out[i].set_low();
        }
        if pressed {
            self.pressed = None;
        } else {
            match self.pressed {
                Some(_) => {}
                None => {
                    self.pressed = Some(Instant::now());
                }
            }
        }
    }
}
