use wasmtime::Caller ;

use crate::startup::{ Plugin, WasmMemSegPtr, WasmMemSegSize, WasmMemorySegment, WasmSendContext };
use crate::extract_wasm_args ;



pub(in crate::exports) fn add_one( num: i32 ) -> i32 {
    num +1
}

pub(in crate::exports) fn print_to_host( mut caller: Caller<Plugin>, ptr: WasmMemSegPtr, size: WasmMemSegSize ) {
    
    let bytes = extract_wasm_args!( caller, ptr, size ).unwrap();
    let _permissions = caller.data().manifest().get_permissions();
    match String::from_utf8( bytes ) {
        Ok(s) => println!( "Wasm print: {}", s ),
        Err(e) => eprintln!( "Error converting to String: {}", e ),
    }

}