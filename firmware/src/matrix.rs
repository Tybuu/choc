use embassy_futures::select::select_array;
use embassy_nrf::{
    gpio::{AnyPin, Output},
    gpiote::{AnyChannel, InputChannel},
};
use embedded_hal::digital::InputPin;

pub struct Matrix<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> {
    out: [Output<'a, AnyPin>; OUTPUT_SIZE],
    input: [InputChannel<'a, AnyChannel, AnyPin>; INPUT_SIZE],
    pressed: bool,
}

impl<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> Matrix<'a, INPUT_SIZE, OUTPUT_SIZE> {
    pub fn new(
        out: [Output<'a, AnyPin>; OUTPUT_SIZE],
        input: [InputChannel<'a, AnyChannel, AnyPin>; INPUT_SIZE],
    ) -> Self {
        Self {
            out,
            input,
            pressed: false,
        }
    }

    // Only returns if a key is high or a previous scan had a key that was high. Otherwise,
    // awaits for a high key
    pub async fn scan(&mut self, states: &mut [[bool; OUTPUT_SIZE]; INPUT_SIZE]) {
        // If no keys were pressed in the previous scan,
        // we'll set all the output pins high and await
        // for one of the channels to go high to save battery
        if !self.pressed {
            for power in &mut self.out {
                power.set_high();
            }

            let mut high = false;
            for row in &mut self.input {
                high = high || row.is_high().unwrap()
            }

            if !high {
                select_array([
                    self.input[0].wait(),
                    self.input[1].wait(),
                    self.input[2].wait(),
                    self.input[3].wait(),
                ])
                .await;
            }

            for power in &mut self.out {
                power.set_low();
            }
        }

        let mut pressed = false;
        for i in 0..OUTPUT_SIZE {
            self.out[i].set_high();
            for j in 0..INPUT_SIZE {
                states[j][i] = self.input[j].is_high().unwrap();
                pressed = pressed || states[j][i];
            }
            self.out[i].set_low();
        }
        self.pressed = pressed;
    }
}
