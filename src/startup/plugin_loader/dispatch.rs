mod dispatch_instruction ;
mod dispatch ;
mod memory_segment ;

pub use dispatch_instruction::FunctionDispatchInstruction ;
use memory_segment::{ WasmMemorySegment, RawMemorySegment, MemoryReadError, MemoryWriteError };