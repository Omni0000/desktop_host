use wasmtime::{ AsContextMut, Caller, Memory, MemoryAccessError, TypedFunc } ;
use thiserror::Error ;

use super::super::ActivePlugin ;




#[derive( Error, Debug )]
pub enum MemorySendError {
    #[error("")] AllocError( #[from] __AllocError ),
    #[error("")] MissingMemoryExport( String ),
    #[error("")] MemoryAccessError( #[from] MemoryAccessError ),
}

#[derive( Error, Debug )]
pub enum __AllocError {
    #[error( "NoOrInvalidAllocExport: {0}" )] NoOrInvalidAllocExport( wasmtime::Error ),
    #[error( "Plugin Exception: {0}" )] PluginException( wasmtime::Error ),
    #[error( "Data Too Large: {0}" )] DataTooLarge( #[from] std::num::TryFromIntError ),
}

#[derive( Error, Debug )]
pub enum MemoryReadError {
    #[error("")] DeallocError( #[from] __DeallocError ),
    #[error("")] MissingMemoryExport( String ),
    #[error("")] MemoryAccessError( #[from] MemoryAccessError ),
}

#[derive( Error, Debug )]
pub enum __DeallocError {
    #[error( "NoOrInvalidDeallocExport: {0}" )] NoOrInvalidDeallocExport( wasmtime::Error ),
    #[error( "Plugin Exception: {0}" )] PluginException( wasmtime::Error ),
}

pub type WasmMemSegPtr = i32 ;
pub type WasmMemSegSize = i32 ;
pub type RawMemorySegment = i64 ;
pub struct WasmMemorySegment {
    offset: usize,
    size: usize,
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

// NOTE: only exported to plugin_loader
pub trait WasmRuntimeContext {

    const ALLOC_FN: &'static str = "alloc" ;
    const DEALLOC_FN: &'static str = "dealloc" ;
    const EXPORTED_MEM_NAME: &'static str = "memory" ;

    fn context_mut( &mut self ) -> impl AsContextMut ;
    fn memory( &mut self, name: &str ) -> Option<Memory> ;
    fn get_typed_func<Args, Results>( &mut self, name: &str ) -> Result<TypedFunc<Args, Results>, wasmtime::Error> where 
        Args: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    ;

    fn send_data( &mut self, data: &[u8]) -> Result<WasmMemorySegment, MemorySendError>
    where
        Self: Sized,
    {

        let memory_segment = self.alloc( data.len() )?;
        self.memory( Self::EXPORTED_MEM_NAME )
            .ok_or_else(|| MemorySendError::MissingMemoryExport( Self::EXPORTED_MEM_NAME.to_string() ))?
            .write( &mut self.context_mut(), memory_segment.offset, data)?;
        Ok( memory_segment )

    }

    fn read_data(&mut self, location: RawMemorySegment) -> Result<Vec<u8>, MemoryReadError> where
        Self: Sized,
    {
        let memory_segment: WasmMemorySegment = location.into();
        let mut buffer: Vec<u8> = vec![1; memory_segment.size];

        self.memory( Self::EXPORTED_MEM_NAME )
            .ok_or_else(|| MemoryReadError::MissingMemoryExport( Self::EXPORTED_MEM_NAME.to_string() ))?
            .read( &mut self.context_mut(), memory_segment.offset, &mut buffer )?;

        self.dealloc( memory_segment )
            .map_err( |err| err.1 )?;

        Ok( buffer )

    }

    fn alloc( &mut self, size: usize ) -> Result<WasmMemorySegment, __AllocError> {
        
        let send_size: WasmMemSegSize = size.try_into()?;
        let offset = self
            .get_typed_func::<WasmMemSegSize, WasmMemSegPtr>( Self::ALLOC_FN )
            .map_err(|e| __AllocError::NoOrInvalidAllocExport( e ))?
            .call(&mut self.context_mut(), send_size)
            .map_err( __AllocError::PluginException )?;
        Ok( WasmMemorySegment::new_unchecked( offset, send_size ))

    }

    fn dealloc( &mut self, segment: WasmMemorySegment ) -> Result<(), ( WasmMemorySegment, __DeallocError )> {
        
        let dealloc_fn = match self.get_typed_func::<RawMemorySegment, ()>( Self::DEALLOC_FN ) {
            Ok( dealloc_fn ) => dealloc_fn,
            Err( err ) => return Err(( segment, __DeallocError::NoOrInvalidDeallocExport( err ) ))
        };

        match dealloc_fn.call( &mut self.context_mut(), segment.as_send()) {
            Ok( _ ) => Ok(()),
            Err( err ) => Err(( segment, __DeallocError::PluginException( err )))
        }
        
    }

}

impl WasmRuntimeContext for ActivePlugin {

    fn context_mut( &mut self ) -> impl AsContextMut {
        self.store.as_context_mut()
    }

    fn memory( &mut self, name: &str ) -> Option<Memory> {
        self.instance.get_memory( &mut self.store, name )
    }

    fn get_typed_func<Args, Results>(
        &mut self,
        name: &str,
    ) -> Result<TypedFunc<Args, Results>, wasmtime::Error>
    where
        Args: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        self.instance.get_typed_func( &mut self.store, name )
    }

}

impl<'a, T> WasmRuntimeContext for Caller<'a, T> {

    fn context_mut( &mut self ) -> impl AsContextMut { self }

    fn memory( &mut self, name: &str ) -> Option<Memory> {
        self.get_export( name ).and_then(| export | export.into_memory())
    }

    fn get_typed_func<Args, Results>(
        &mut self,
        name: &str,
    ) -> Result<TypedFunc<Args, Results>, wasmtime::Error>
    where
        Args: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        self.get_export( name )
            .and_then(| export | export.into_func())
            .ok_or_else(|| wasmtime::Error::msg( format!( "Export '{name}' not found or not a function" )))
            .and_then(| func | func.typed( self ))
    }

}