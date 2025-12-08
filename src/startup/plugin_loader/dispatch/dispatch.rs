use thiserror::Error ;
use itertools::Itertools ;

use crate::startup::InterfaceId ;
use super::super::{ LivePluginTree };
use super::FunctionDispatchInstruction ;
use super::{ RawMemorySegment, WasmMemorySegment, MemoryReadError, MemorySendError };
use super::WasmSendContext ;



#[derive( Error, Debug )]
#[error( "Invalid Socket: '{0}'" )] pub struct InvalidSocketError( InterfaceId );

#[derive( Error, Debug )]
pub enum DispatchError {
    #[error( "Deadlock" )] Deadlock,
    #[error( "Dispatch Failure: {0}" )] DispatchFailure( #[from] wasmtime::Error ),
    #[error( "Memory Read Error: {0}" )] MemoryReadError( #[from] MemoryReadError ),
    #[error( "Memory Write Error: {0}" )] MemoryWriteError( #[from] MemorySendError ),
}

impl LivePluginTree {

    pub fn dispatch_function<'a>(
        &'a self,
        instruction: FunctionDispatchInstruction,
        params: &[u8],
    ) -> Result<(
        Vec<Result< Vec<u8>, DispatchError >>,
        Vec<wasmtime::Error>
    ), InvalidSocketError > {
        
        Ok( self.socket_map.get( &instruction.socket )
            .ok_or( InvalidSocketError( instruction.socket.clone()) )?
            .into_iter()
            .map(|( _, rw_plugin )| {

                let mut plugin_lock = match rw_plugin.write() {
                   Ok( lock ) => lock,
                   _ => return Ok( Err( DispatchError::Deadlock )), 
                };

                let active_plugin = plugin_lock.load( &self.engine, &self.linker )?;

                Ok( dispatch_function_of( active_plugin, &instruction.function, params ))

            })
            .partition_result()
        )

    }

}

pub(in super::super) fn dispatch_function_of( plugin: &mut impl WasmSendContext, function: &String, data: &[u8] ) -> Result< Vec<u8>, DispatchError > {

    let params_memory_segment: WasmMemorySegment = plugin.send_data( &data )?;
    let response_memory_segment: RawMemorySegment = plugin
        .get_typed_func( function )?
        .call( &mut plugin.context_mut(), params_memory_segment.as_send() )?;
    Ok( plugin.read_data( response_memory_segment )? )

}