use wasmtime::{ MemoryAccessError };
use thiserror::Error ;

use super::super::ActivePlugin ;



#[derive( Error, Debug )]
pub enum MemoryWriteError {
    #[error( "Allocation Error: {0}")] AllocationError( wasmtime::Error ),
    #[error( "Memory Access Error: {0}" )] MemoryAccessError( #[from] MemoryAccessError ),
    #[error( "Missing Memory Export: '{0}'" )] MissingMemoryExport( String ),
    #[error( "Data Too Large: '{0}'")] DataTooLarge( std::num::TryFromIntError ),
}
#[derive( Error, Debug )]
enum __AllocError {
    #[error( "{0}" )] AllocError( #[from] wasmtime::Error ),
    #[error( "{0}" )] DataTooLarge( #[from] std::num::TryFromIntError ),
}
impl From<__AllocError> for MemoryWriteError {
    fn from( err: __AllocError ) -> Self {
        match err {
            __AllocError::AllocError( err ) => Self::AllocationError( err ),
            __AllocError::DataTooLarge( err ) => Self::DataTooLarge( err ),
        }
    }
}

#[derive( Error, Debug )]
pub enum MemoryReadError {
    #[error( "Deallocation Error: {0}")] DeallocationError( #[from] wasmtime::Error ),
    #[error( "Memory Access Error: {0}" )] MemoryAccessError( #[from] MemoryAccessError ),
    #[error( "Missing Memory Export: '{0}'" )] MissingMemoryExport( String ),
}

type SendPtr = u32 ;
type SendSize = u32 ;
pub(super) type RawMemorySegment = u64 ;
pub(super) struct WasmMemorySegment {
    offset: usize,
    size: usize,
}
impl WasmMemorySegment {
    pub fn new_unchecked( offset: SendPtr, size: SendSize ) -> Self {
        Self { offset: offset as usize, size: size as usize }
    }
    pub fn as_send( &self ) -> RawMemorySegment {
        (( self.offset as u64 ) << 32 ) | ( self.size as u64 )
    }
}
impl From<RawMemorySegment> for WasmMemorySegment {
    fn from( raw: RawMemorySegment ) -> Self {
        WasmMemorySegment::new_unchecked( ( raw >> 32 ) as u32, raw as u32 )
    }   
}


impl ActivePlugin {

    const ALLOC_FN: &'static str = "alloc" ;
    const DEALLOC_FN: &'static str = "dealloc" ;
    const EXPORTED_MEM_NAME: &'static str = "memory" ;

    pub(super) fn send_data( &mut self, data: &[u8]) -> Result<WasmMemorySegment, MemoryWriteError> {
        
        let memory_segment = self.alloc( data.len() )?;

        self.instance
            .get_memory( &mut self.store, Self::EXPORTED_MEM_NAME )
            .ok_or( MemoryWriteError::MissingMemoryExport( Self::EXPORTED_MEM_NAME.to_string() ) )?
            .write( &mut self.store, memory_segment.offset as usize, data )?;

        Ok( memory_segment )

    }

    pub(super) fn read_data( &mut self, location: RawMemorySegment ) -> Result<Vec<u8>, MemoryReadError> {

        let memory_segment: WasmMemorySegment = location.into();
        let mut buffer: Vec<u8> = vec![ 1; memory_segment.size as usize ];

        self.instance
            .get_memory( &mut self.store, Self::EXPORTED_MEM_NAME )
            .ok_or( MemoryReadError::MissingMemoryExport( Self::EXPORTED_MEM_NAME.to_string() ) )?
            .read( &mut self.store, memory_segment.offset as usize, &mut buffer )?;

        self.dealloc( memory_segment ).map_err(| err | err.1 )?;

        Ok( buffer )

    }

    fn alloc( &mut self, size: usize ) -> Result<WasmMemorySegment, __AllocError> {
        let send_size: SendSize = size.try_into()?;
        let offset: SendPtr = self.instance
            .get_typed_func::<SendSize, SendPtr>( &mut self.store, Self::ALLOC_FN )?
            .call( &mut self.store, send_size )?;
        Ok( WasmMemorySegment::new_unchecked( offset, send_size ))
    }

    fn dealloc( &mut self, segment: WasmMemorySegment ) -> Result<(), ( WasmMemorySegment, wasmtime::Error )> {
        
        let deallocate_fn = match self.instance
            .get_typed_func::<RawMemorySegment,()>( &mut self.store, Self::DEALLOC_FN )
        {
            Ok( deallocate_fn ) => deallocate_fn,
            Err( err ) => return Err(( segment, err.into() )),
        };

        match deallocate_fn.call( &mut self.store, segment.as_send() ) {
            Ok( _ ) => {},
            Err( err ) => return Err(( segment, err.into() )),
        };

        Ok(())

    }

}