use embassy_nrf::twim::{self, Instance, Twim};
use embassy_time::Timer;

const SLAVE_ADDR: u8 = 0x2A;

const WRITE_MASK: u8 = 0x80;
const READ_MASK: u8 = 0xA0;

const SYS_CONFIG1_ADDR: u8 = 0x03;

const FEED_CONFIG1_ADDR: u8 = 0x04;
const FEED_CONFIG1_RELATIVE: u8 = 0b11000001;

const FEED_CONFIG2_ADDR: u8 = 0x05;

const FLAGS: u8 = 0x02;

#[derive(Debug, Default)]
pub struct AbsoluteDataPacket {
    pub x: u16,
    pub y: u16,
    z: u16,
    button_flags: u8,
    touch_down: bool,
    hovering: bool,
}

pub struct TrackPad<'a, 'b, T: Instance> {
    i2c: &'a mut Twim<'b, T>,
    pub data: AbsoluteDataPacket,
}

impl<'a, 'b, T: Instance> TrackPad<'a, 'b, T> {
    pub async fn new(i2c: &'a mut Twim<'b, T>) -> Self {
        let mut dev = Self {
            i2c,
            data: AbsoluteDataPacket::default(),
        };

        let register = WRITE_MASK | SYS_CONFIG1_ADDR;
        dev.i2c.write(SLAVE_ADDR, &[register, 0b001]).await.unwrap();

        Timer::after_millis(50).await;

        let register = WRITE_MASK | SYS_CONFIG1_ADDR;
        dev.i2c.write(SLAVE_ADDR, &[register, 0b000]).await.unwrap();

        let register = WRITE_MASK | FEED_CONFIG1_ADDR;
        dev.i2c
            .write(SLAVE_ADDR, &[register, FEED_CONFIG1_RELATIVE])
            .await
            .unwrap();

        let register = WRITE_MASK | FEED_CONFIG2_ADDR;
        dev.i2c.write(SLAVE_ADDR, &[register, 0x0]).await.unwrap();

        dev
    }

    pub async fn sleep(&mut self, input: bool) {
        if input {
            let register = WRITE_MASK | SYS_CONFIG1_ADDR;
            self.i2c.write(SLAVE_ADDR, &[register, 0b100]).await;
            // .unwrap();
        } else {
            let register = WRITE_MASK | SYS_CONFIG1_ADDR;
            self.i2c.write(SLAVE_ADDR, &[register, 0b000]).await;
            // .unwrap();
        }
    }

    async fn clear_flags(&mut self) {
        let register = WRITE_MASK | FLAGS;
        self.i2c
            .write(SLAVE_ADDR, &[register, 0x000])
            .await
            .unwrap();
        Timer::after_micros(50).await;
    }

    async fn data_ready(&mut self) -> bool {
        let register = READ_MASK | FLAGS;
        let mut buf = [0u8];
        self.i2c.write_read(SLAVE_ADDR, &[register], &mut buf).await;
        // .unwrap();

        if buf[0] & 0b1100 != 0 {
            true
        } else {
            false
        }
    }

    pub async fn get_relative(&mut self) -> Option<((i8, i8, u8))> {
        if self.data_ready().await {
            let mut buf = [0u8; 4];
            let register = READ_MASK | 0x12;
            self.i2c
                .write_read(SLAVE_ADDR, &[register], &mut buf)
                .await
                .unwrap();
            let mut x = buf[1] as i8;
            let mut y = buf[2] as i8;
            let button = buf[0] & 1;
            self.clear_flags().await;
            Some((x, y, button))
        } else {
            None
        }
    }
}
