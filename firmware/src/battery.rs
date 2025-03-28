use embassy_nrf::saadc::Saadc;

fn get_percentage(voltage: f32) -> u8 {
    let mv = (voltage * 1000f32) as u16;
    if mv >= 4200 {
        100
    } else if mv <= 3450 {
        0
    } else {
        ((mv * 2) / 15 - 459) as u8
    }
}

pub struct BatteryVoltage<'a, 'b, const N: usize> {
    saadc: &'b mut Saadc<'a, N>,
    current_percentage: u8,
    index: usize,
}

impl<'a, 'b, const N: usize> BatteryVoltage<'a, 'b, N> {
    pub async fn new(saadc: &'b mut Saadc<'a, N>, index: usize) -> Self {
        let mut buf = [0i16; N];
        saadc.calibrate().await;
        Self {
            saadc,
            current_percentage: 0,
            index,
        }
    }

    /// Only returns the previous percentage reading. Doesn't start new reading
    pub fn get_reading(&self) -> u8 {
        self.current_percentage
    }

    /// Does a new reading a returns a a percentage if the value changed
    pub async fn update_reading(&mut self) -> Option<u8> {
        let mut buf = [0i16; N];
        self.saadc.sample(&mut buf).await;
        let reading = buf[self.index];
        // let current_percentage = get_percentage(reading as f32 * 0.6 * 2.0 * 5.0 / 4095.0);
        let current_percentage =
            get_percentage(((reading as f32 * 0.6) / 4095.0) * 1100000.0 / 100000.0);
        if self.current_percentage != current_percentage {
            self.current_percentage = current_percentage;
            Some(self.current_percentage)
        } else {
            None
        }
    }
}
