//SPDX-License-Identifier: GPL-2.0

//! Driver for Boradcom BCM2835 GPIO unit (pinctrl + GPIO)
//!
//! Based on the C driver

use core::result::Result::Ok;

use kernel::{
    bit, define_of_id_table, device, gpio,
    io_mem::IoMem,
    module_platform_driver, of, platform,
    prelude::*,
    sync::{Arc, ArcBorrow},
};

macro_rules! FSEL_REG {
    ($p:expr) => {
        GPFSEL0 + (($p / 10) * 4)
    };
}

macro_rules! FSEL_SHIFT {
    ($p:expr) => {
        (($p % 10) * 3)
    };
}

macro_rules! GPIO_REG_OFFSET {
    ($p:expr) => {
        $p / 32
    };
}

macro_rules! GPIO_REG_SHIFT {
    ($p:expr) => {
        $p % 32
    };
}

//GPIO register offsets
const GPFSEL0: usize = 0x0; //function select
const GPSET0: usize = 0x1c; //pin output set
const GPCLR0: usize = 0x28; //pin output clear
const GPLEV0: usize = 0x34; //pin level
                            // const GPEDS0:usize = 0x40; //pin event detect Status
                            // const GPREN0:usize = 0x4c; //pin rising edge detect enable
                            // const GPFEN0:usize = 0x58; //pin falling edge detect enable
                            // const GPHEN0:usize = 0x64; //pin high detect enable
                            // const GPLEN0:usize = 0x70; //pin low detect enable
                            // const GPAREN0:usize= 0x7c; //pin async rising edge detect
                            // const GPAFEN0:usize= 0x88; // pin async falling edge detect
                            // const GPPUD:usize = 0x94;  //pin pull-up/down enable
                            // const GPPUDCLK0:usize = 0x98; // pin pull-up/down enable clock
                            //TODO: no sure the precise offset size of BCM2835
const GPIO_SIZE: usize = 0x1000;

const BCM2835_NUM_GPIOS: u16 = 54;
// const BCM2835_NUM_BANKS:usize  = 2;
// const BCM2835_NUM_IRQS:usize = 3;

// bcm2835_fsel
// const BCM2835_FSEL_COUNT:usize = 8;
const BCM2835_FSEL_MASK: u32 = 0x7;
// brcm, function property
const BCM2835_FSEL_GPIO_IN: u32 = 0;
const BCM2835_FSEL_GPIO_OUT: u32 = 1;
// const BCM2835_FSEL_ALT5:u32     = 2;
// const BCM2835_FSEL_ALT4:u32     = 3;
// const BCM2835_FSEL_ALT0:u32     = 4;
// const BCM2835_FSEL_ALT1:u32     = 5;
// const BCM2835_FSEL_ALT2:u32     = 6;
// const BCM2835_FSEL_ALT3:u32     = 7;

// const BCM2835_FUNCTIONS:[&str;BCM2835_FSEL_COUNT]= [
//     "gpio_in",
//     "gpio_out",
//     "alt0",
//     "alt1",
//     "alt2",
//     "alt3",
//     "alt4",
//     "alt5",
// ];

// struct BCM2835DataInner{
//     //TODO: data in bcm2835
// }

struct BCM2835Resources {
    base: IoMem<GPIO_SIZE>,
    // wake_irq:&'a[i32],
    // enablied_irq_map:[u64;BCM2835_NUM_BANKS],
    // irq_type:[u32;BCM2835_NUM_GPIOS],
}

struct BCM2835Data {
    dev: device::Device,
    // inner: RawSpinLock<BCM2835DataInner>,
}

type BCM2835Registrations = gpio::Registration<BCM2835Device>;

type DeviceData = device::Data<BCM2835Registrations, BCM2835Resources, BCM2835Data>;

struct BCM2835Device;

impl BCM2835Device {
    #[inline]
    fn bcm2835_gpio_rd(data: ArcBorrow<'_, DeviceData>, reg: usize) -> Result<u32> {
        let bcm2835 = data.resources().ok_or(ENXIO)?;
        bcm2835.base.try_readl(reg)
    }

    #[inline]
    fn bcm2835_gpio_wr(data: ArcBorrow<'_, DeviceData>, reg: usize, val: u32) -> Result {
        let bcm2835 = data.resources().ok_or(ENXIO)?;
        bcm2835.base.try_writel(val, reg)?;
        Ok(())
    }

    #[inline]
    fn bcm2835_gpio_get_bit(
        data: ArcBorrow<'_, DeviceData>,
        reg: usize,
        offset: u32,
    ) -> Result<bool> {
        let reg = reg + GPIO_REG_OFFSET!(offset as usize) * 4;
        Ok(((Self::bcm2835_gpio_rd(data, reg)? >> (GPIO_REG_SHIFT!(offset))) & 1) != 0)
    }

    #[inline]
    fn bcm2835_gpio_set_bit(data: ArcBorrow<'_, DeviceData>, reg: usize, offset: u32) -> Result {
        let reg = reg + GPIO_REG_OFFSET!(offset as usize) * 4;
        let val = bit(GPIO_REG_SHIFT!(offset)).into();
        Self::bcm2835_gpio_wr(data, reg, val)?;
        Ok(())
    }

    #[inline]
    fn bcm2835_pinctrl_fsel_get(data: ArcBorrow<'_, DeviceData>, pin: usize) -> Result<u32> {
        let val = Self::bcm2835_gpio_rd(data, FSEL_REG!(pin))?;
        let status = (val >> FSEL_SHIFT!(pin as u32)) & BCM2835_FSEL_MASK;
        Ok(status)
    }

    #[inline]
    fn bcm2835_pinctrl_fsel_set(data: ArcBorrow<'_, DeviceData>, pin: usize, fsel: u32) -> Result {
        let mut val = Self::bcm2835_gpio_rd(data, FSEL_REG!(pin))?;
        let cur = (val >> FSEL_SHIFT!(pin as u32)) & BCM2835_FSEL_MASK;

        // dev_dbg!(data.dev,"read{}({}=>{}\n)",val,pin,BCM2835_FUNCTIONS[cur]);

        if cur == fsel {
            return Ok(());
        }

        if cur != BCM2835_FSEL_GPIO_IN && fsel != BCM2835_FSEL_GPIO_IN {
            val &= !(BCM2835_FSEL_MASK << FSEL_SHIFT!(pin as u32));
            val |= fsel << FSEL_SHIFT!(pin as u32);

            // dev_dbg!(data.dev,"trans {} ({} <= {})\n",val,pin,BCM2835_FUNCTIONS[BCM2835_FSEL_GPIO_IN as usize]);
            Self::bcm2835_gpio_wr(data, FSEL_REG!(pin), val)?;
        }

        val &= !(BCM2835_FSEL_MASK << FSEL_SHIFT!(pin as u32));
        val |= fsel << FSEL_SHIFT!(pin as u32);

        // dev_dbg!(data,"write {} ({}<={})\n",val,pin,BCM2835_FUNCTIONS[fsel]);
        Self::bcm2835_gpio_wr(data, FSEL_REG!(pin), val)?;
        Ok(())
    }
}

//TODO: implement the items in trait gpio::Chip
#[vtable]
impl gpio::Chip for BCM2835Device {
    type Data = Arc<DeviceData>;

    fn get_direction(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result<gpio::LineDirection> {
        // let bcm2835_pinctrl = data.resources().ok_or(ENXIO)?;
        let fsel = Self::bcm2835_pinctrl_fsel_get(data, offset as usize)?;

        //Alternative function doesn't clearly provide a direction
        if fsel > BCM2835_FSEL_GPIO_OUT {
            //FIXME: Err(EINVAL)
            return Err(ENOTSUPP);
        }

        Ok(if fsel == BCM2835_FSEL_GPIO_IN {
            gpio::LineDirection::In
        } else {
            gpio::LineDirection::Out
        })
    }

    fn direction_input(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result {
        // let bcm2835_pinctrl = data.resources().ok_or(ENXIO);
        Self::bcm2835_pinctrl_fsel_set(
            data,
            offset as usize,
            BCM2835_FSEL_GPIO_IN,
        )
    }

    fn direction_output(data: ArcBorrow<'_, DeviceData>, offset: u32, value: bool) -> Result {
        let reg = if value { GPSET0 } else { GPCLR0 };
        Self::bcm2835_gpio_set_bit(data, reg, offset)?;
        Self::bcm2835_pinctrl_fsel_set(data, offset as usize, BCM2835_FSEL_GPIO_OUT)?;
        Ok(())
    }

    fn set(data: ArcBorrow<'_, DeviceData>, offset: u32, value: bool) {
        let reg = if value { GPSET0 } else { GPCLR0 };
        let _ = Self::bcm2835_pinctrl_fsel_set(data, reg, offset);
    }

    fn get(data: ArcBorrow<'_, DeviceData>, offset: u32) -> Result<bool> {
        Self::bcm2835_gpio_get_bit(data, GPLEV0, offset)
    }
}

impl platform::Driver for BCM2835Device {
    type Data = Arc<DeviceData>;

    define_of_id_table! {(),[
        //FIXME: None is likely not correct, should fix it maybe
        (of::DeviceId::Compatible(b"brcm,bcm2835-gpio"),None),
    ]}

    fn probe(dev: &mut platform::Device, _data: Option<&Self::IdInfo>) -> Result<Arc<DeviceData>> {
        let res = dev.res().ok_or(ENXIO)?;

        let data = kernel::new_device_data!(
            gpio::Registration::new(),
            BCM2835Resources {
                //SAFETY:
                base: unsafe { IoMem::try_new(res)? },
            },
            BCM2835Data {
                dev: device::Device::from_dev(dev),
            },
            "BCM2835::Regsiterations"
        )?;

        let data = Arc::<DeviceData>::from(data);

        kernel::gpio_chip_register!(
            data.registrations().ok_or(ENXIO)?.as_pinned_mut(),
            BCM2835_NUM_GPIOS,
            None,
            dev,
            data.clone()
        )?;

        dev_info!(data.dev, "RUST BCM2835 GPIO CHIP registered!!!\n");

        Ok(data)
    }
}

module_platform_driver! {
    type: BCM2835Device,
    name: "pinctrl_bcm2835_rust",
    author: "Tianyu She",
    description: "BCM2835 GPIO Part",
    license: "GPL",
}
