//SPDX-License-Identifier: GPL-2.0

//! Driver for Boradcom BCM2835 GPIO unit (pinctrl + GPIO) 
//! 
//! Based on the C driver


use core::result::Result::Ok;

use kernel::{
    bit,
    prelude::*,
    gpio,device,
    io_mem::IoMem, sync::{RawSpinLock, ArcBorrow}, amba::Device,
    platform, define_of_id_table,
};

macro_rules! FSEL_REG{
    ($p:expr) => {
        GPFSEL0 + (($p/10) * 4)
    };
}

macro_rules! FSEL_SHIFT {
    ($p:expr) => {
        ($p%10)*3
    }
}

macro_rules! GPIO_REG_OFFSET {
    ($p:expr) => {
        $p / 32
    }
}

macro_rules! GPIO_REG_SHIFT {
    ($p:expr) => {
        $p % 32
    }
}

//GPIO register offsets
const GPFSEL0:usize = 0x0; //function select
const GPSET0:usize = 0x1c; //pin output set
const GPCLR0:usize = 0x28; //pin output clear
const GPLEV0:usize = 0x34; //pin level
const GPEDS0:usize = 0x40; //pin event detect Status
const GPREN0:usize = 0x4c; //pin rising edge detect enable
const GPFEN0:usize = 0x58; //pin falling edge detect enable
const GPHEN0:usize = 0x64; //pin high detect enable
const GPLEN0:usize = 0x70; //pin low detect enable
const GPAREN0:usize= 0x7c; //pin async rising edge detect
const GPAFEN0:usize= 0x88; // pin async falling edge detect
const GPPUD:usize = 0x94;  //pin pull-up/down enable
const GPPUDCLK0:usize = 0x98; // pin pull-up/down enable clock
//TODO: no sure the precise offset size of BCM2835
const GPIO_SIZE:usize= 0x1000;   

const BCM2835_NUM_GPIOS:usize = 54;
const BCM2835_NUM_BANKS:usize  = 2;
const BCM2835_NUM_IRQS:usize = 3;

// bcm2835_fsel
const BCM2835_FSEL_COUNT:usize= 8;
const BCM2835_FSEL_MASK:u32 = 0x7;
// brcm, function property
const BCM2835_FSEL_GPIO_IN:u32 =0;
const BCM2835_FSEL_GPIO_OUT:u32 =1;
const BCM2835_FSEL_ALT5:u32 =2;
const BCM2835_FSEL_ALT4:u32 =3;
const BCM2835_FSEL_ALT0:u32 =4;
const BCM2835_FSEL_ALT1:u32 =5;
const BCM2835_FSEL_ALT2:u32 =6;
const BCM2835_FSEL_ALT3:u32 =7;

const BCM2835_FUNCTIONS:[&str;BCM2835_FSEL_COUNT]= [
    "gpio_in",
    "gpio_out",
    "alt0",
    "alt1",
    "alt2",
    "alt3",
    "alt4",
    "alt5",
];

struct BCM2835DataInner{
    //TODO: data in bcm2835
}

struct BCM2835Resources<'a>{
    base: IoMem<GPIO_SIZE>,
    wake_irq:&'a[i32],
    enablied_irq_map:[u64;BCM2835_NUM_BANKS],
    irq_type:[u32;BCM2835_NUM_GPIOS],
    
}

struct BCM2835Data{
    dev: device::Device,
    inner: RawSpinLock<BCM2835DataInner>,
}

type BCM2835Registrations = gpio::Registration<BCM2835Device>;

type DeviceData = device::Data<BCM2835Device,BCM2835Resources,BCM2835Data>;


struct BCM2835Device;

impl BCM2835Device {
    #[inline]
    fn bcm2835_gpio_rd(data:ArcBorrow<'_, DeviceData>, reg:u32)-> Result{
        let bcm2835 = data.resources().ok_or(ENXIO)?;
        Ok(bcm2835.base.readl(reg))
    }

    #[inline]
    fn bcm2835_gpio_wr(data:ArcBorrow<'_, DeviceData>,reg:u32,val:u32)->Result{
        let bcm2835 = data.resources().ok_or(ENXIO)?;
        bcm2835.base.writel(val,reg);
        Ok(())
    }

    #[inline]
    fn bcm2835_gpio_get_bit(data:ArcBorrow<'_, DeviceData>,reg:u32,bit:u32)->Result{
        reg += GPIO_REG_OFFSET(bit)*4;
        Ok((bcm2835_gpio_rd(data,reg)>> GPIO_REG_SHIFT(bit) & 1))
    }   

    #[inline]
    fn bcm2835_gpio_set_bit(data:ArcBorrow<'_, DeviceData>,reg:u32,bit:u32)->Result{
        reg += GPIO_REG_OFFSET(bit)*4;
        bcm2835_gpio_wr(data,reg,BIT(GPIO_REG_SHIFT(bit)));
        ok(())
    }

    #[inline]
    fn bcm2835_pinctrl_fsel_get(data:ArcBorrow<'_, DeviceData>,pin:u32)->Result{
        let val: u32 = bcm2835_gpio_rd(data,FSEL_REG!(pin))?;
        let status = (val >> FSEL_SHIFT!(pin)) & BCM2835_FSEL_MASK;
        Ok(status)
    }

    #[inline]
    fn bcm2835_pinctrl_fsel_set(data:ArcBorrow<'_,DeviceData>,pin:u32,fsel:u32)->Result{
        let val:u32 = bcm2835_gpio_rd(data,FSEL_REG!(pin))?;
        let cur:u32 = (val>>FSEL_SHIFT!(pin))& BCM2835_FSEL_MASK;

        dev_dbg!(data.dev,"read{}({}=>{}\n)",val,pin,BCM2835_FUNCTIONS[cur as usize]);

        if(cur ==fsel){
            Ok(())
        }

        if(cur != BCM2835_FSEL_GPIO_IN && fsel != BCM2835_FSEL_GPIO_IN){
            val &= !(BCM2835_FSEL_MASK << FSEL_SHIFT!(pin));
            val |= fsel << FSEL_SHIFT!(pin);

            dev_dbg!(data.dev,"trans {} ({} <= {})\n",val,pin,
                    BCM2835_FUNCTIONS[BCM2835_FSEL_GPIO_IN as usize]);
            bcm2835_gpio_wr(data,FSEL_REG!(pin),val);
        }   
        
        val &= !(BCM2835_FSEL_MASK << FSEL_SHIFT!(pin));
        val |= fsel << FSEL_SHIFT!(pin);

        dev_dbg(data,"write {} ({}<={})\n",val,pin,
                BCM2835_FUNCTIONS[fsel as usize]);
        bcm2835_gpio_wr(data,FSEL_REG!(pin),val);
        Ok(())
    }
}


//TODO: implement the items in trait gpio::Chip
#[vtable]
impl gpio::Chip for BCM2835Device {
    type Data = Arc<DeviceData>;

    fn get_direction(data:ArcBorrow<'_, DeviceData>,offset:u32)->Result<gpio::LineDirection>{
        // let bcm2835_pinctrl = data.resources().ok_or(ENXIO)?;
        let fsel = Self::bcm2835_pinctrl_fsel_get(data, offset)?;

        //Alternative function doesn't clearly provide a direction
        if fsel > BCM2835_FSEL_GPIO_OUT {
            //FIXME: Err(EINVAL)
            return Err(ENOTSUPP);  
        }

        Ok(if fsel == BCM2835_FSEL_GPIO_IN{
            gpio::LineDirection::In
        }else{
            gpio::LineDirection::Out
        })
    }

    fn direction_input(data:ArcBorrow<'_,DeviceData>,offset:u32)->Result{
        // let bcm2835_pinctrl = data.resources().ok_or(ENXIO); 
        ok(Self::bcm2835_pinctrl_fsel_set(data,offset,BCM2835_FSEL_GPIO_IN)?)
    }

    fn direction_output(data:ArcBorrow<'_,DeviceData>,offset:u32,value:bool)->Result{
        let reg = if value {GPSET0} else {GPCLR0};
        Self::bcm2835_gpio_set_bit(data,reg, offset);
        Self::bcm2835_pinctrl_fsel_set(data,offset, BCM2835_FSEL_GPIO_OUT);
        Ok(())
    }

    fn set(data:ArcBorrow<'_,DeviceData>,offset:u32,value:bool){
        let reg = if value {GPSET0} else {GPCLR0};
        Self::bcm2835_pinctrl_fsel_set(data, reg, offset);
    }

    fn get(data:ArcBorrow<'_, DeviceData>,offset:u32)->Result<bool>{
        let val = Self::bcm2835_gpio_get_bit(data, GPLEV0, offset);
        Ok(val != 0)
    }
}

impl platform::Driver for BCM2835Device{
    type Data = Arc<DeviceData>;

    define_of_id_table! {(),[
        //FIXME: None is likely not correct, should fix it maybe
        (of::DeviceId::Compatible(b"brcm,bcm2835-gpio"),None),
    ]}

    fn probe(dev:&mut platform::Device,_data:Option<&Self::IdInfo>)-> Result<Arc<DeviceData>>{
        let res = unsafe { &*dev.ptr };

    }

}

