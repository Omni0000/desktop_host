
pub type WasmMemSegPtr = i32 ;
pub type WasmMemSegSize = i32 ;
pub type RawMemorySegment = i64 ;
pub struct WasmMemorySegment {
    pub(super) offset: usize,
    pub(super) size: usize,
}
impl WasmMemorySegment {
    pub fn new_unchecked( offset: WasmMemSegPtr, size: WasmMemSegSize ) -> Self {
        Self { offset: offset as usize, size: size as usize }
    }
    pub fn as_send( &self ) -> RawMemorySegment {
        (( self.offset as RawMemorySegment ) << 32 ) | ( self.size as RawMemorySegment )
    }
}
impl From<RawMemorySegment> for WasmMemorySegment {
    fn from( raw: RawMemorySegment ) -> Self {
        WasmMemorySegment::new_unchecked( ( raw >> 32 ) as WasmMemSegPtr, raw as WasmMemSegSize )
    }   
}