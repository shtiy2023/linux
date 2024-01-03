use kernel::prelude::*;

// module!是一个宏，用于声明内核模块，所以是必须的
// 该宏必须指定三种参数： ‘type' 'name' 'license'
module! {
    type:RustScull,
    name:"rust_scull",
    author:"BUPT sty",
    description:"RUST Scull Sample",
    license:"GPL",    
}


// same to the module::type
struct RustScull;

// 为RustScull 实现 ‘kernel::Module’ trait
// 该方法 init 相当于 C API宏 “module_init”, 通过这个方法创建实例
impl kernel::Module for RustScull{

    fn init(_name:&'static CStr, _module: &'static ThisModule) -> Result<Self>{
        pr_info!("Rust Scull sample (init)\n");
        Ok(RustScull)
    }

}

impl Drop for RustScull{
    fn drop(&mut self){
        pr_info!("Rust self tests (exit)\n");
    }
}