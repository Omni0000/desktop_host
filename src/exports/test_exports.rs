use capnp::io::Write;
use wasmtime::Caller;

use crate::startup::{ Plugin, WasmMemSegPtr, WasmMemSegSize, WasmMemorySegment, WasmRuntimeContext };



pub(super) fn add_one( num: i32 ) -> i32 {
    num +1
}

pub(super) fn print_to_host( mut caller: Caller<'_, Plugin>, ptr: WasmMemSegPtr, size: WasmMemSegSize ) {
    
    let memory_segment = WasmMemorySegment::new_unchecked( ptr, size );
    let bytes = caller.read_data( memory_segment.as_send() ).unwrap();
    match String::from_utf8( bytes ) {
        Ok(s) => println!( "Wasm print: {}", s ),
        Err(e) => eprintln!( "Error converting to String: {}", e ),
    }

}