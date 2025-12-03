
use thiserror::Error ;

use super::super::{ ActivePlugin, LivePluginTree, PluginTreeNode };
use super::super::load_plugin::LoaderError ;
use super::super::preload_socket::InvalidSocket ;
use super::FunctionDispatchInstruction ;
use super::{ RawMemorySegment, WasmMemorySegment, MemoryReadError, MemoryWriteError };



#[derive( Error, Debug )]
pub enum DispatchError {
    #[error( "Dispatch Failure: {0}" )] DispatchFailure( #[from] wasmtime::Error ),
    #[error( "Memory Read Error: {0}" )] MemoryReadError( #[from] MemoryReadError ),
    #[error( "Memory Write Error: {0}" )] MemoryWriteError( #[from] MemoryWriteError ),
}

impl LivePluginTree {

    pub fn dispatch_function<'a>(
        &'a mut self,
        instruction: FunctionDispatchInstruction,
        params: &[u8],
    ) -> Result<(
        Vec<Result< Vec<u8>, DispatchError >>,
        Vec<LoaderError>
    ), InvalidSocket > {
        
        let preload_errors = self.preload_socket( &instruction.socket )?;
        
        let results = self.socket_map.get_mut( &instruction.socket )
            .ok_or( InvalidSocket( instruction.socket.clone()) )?
            .into_iter()
            .filter_map(| plugin |
                if let PluginTreeNode::ActivePlugin( active_plugin) = plugin {
                    Some( dispatch_function_of( active_plugin, &instruction.function, params ))
                } else { None }
            )
            .collect();


        Ok(( results, preload_errors ))
    }

}

#[inline] fn dispatch_function_of( plugin: &mut ActivePlugin, function: &String, data: &[u8] ) -> Result< Vec<u8>, DispatchError > {

    let params_memory_segment: WasmMemorySegment = plugin.send_data( &data )?;
    let response_memory_segment: RawMemorySegment = plugin.instance
        .get_typed_func::<RawMemorySegment, RawMemorySegment>( &mut plugin.store, function )?
        .call( &mut plugin.store, params_memory_segment.as_send() )?;
    Ok( plugin.read_data( response_memory_segment )? )

}