mod dispatch_instruction ;
mod dispatch ;
mod memory_segment ;
mod wasm_send ;
mod wasm_send_utils ;

pub use dispatch_instruction::FunctionDispatchInstruction ;
// pub( super ) use dispatch::dispatch_function_of ;
pub use memory_segment::{ WasmMemorySegment, WasmMemSegPtr, WasmMemSegSize, RawMemorySegment };
pub use wasm_send::{ WasmSendContext, MemoryReadError, MemorySendError };