mod dispatch_instruction ;
mod dispatch ;
mod memory_segment ;

pub use dispatch_instruction::FunctionDispatchInstruction ;
// pub( super ) use dispatch::dispatch_function_of ;
pub use memory_segment::{ WasmMemorySegment, WasmRuntimeContext, WasmMemSegPtr, WasmMemSegSize, RawMemorySegment, MemoryReadError, MemorySendError };