use std::io::Cursor ;
use capnp::message::{ self, HeapAllocator, ReaderOptions };
use capnp::serialize ;
use wasmtime::Caller ;

use crate::startup::{ InterfaceId, Plugin };
use crate::startup::{ RawMemorySegment, WasmMemSegPtr, WasmMemSegSize, WasmSendContext };
use crate::startup::{ DispatchError, MemoryReadError, MemorySendError };
use crate::capnp::exports::bridge_capnp::{ function_call_instruction, function_call_result, FunctionCallError };
use crate::{ LIVE_PLUGIN_TREE, extract_wasm_args };



pub(in crate::exports) fn call_on_socket(
    mut caller: Caller<Plugin>,
    instruction_ptr: WasmMemSegPtr,
    instruction_size: WasmMemSegSize,
    args_ptr: WasmMemSegPtr,
    args_size: WasmMemSegSize,
) -> RawMemorySegment {
    match (|| {

        let instruction = parse_instruction( &mut caller, instruction_ptr, instruction_size )?;
        let args = extract_wasm_args!( &mut caller, args_ptr, args_size )
            .map_err(| _ | FunctionCallError::InvalidArgsMemorySegment )?;

        let ( results, load_errors ) = LIVE_PLUGIN_TREE.dispatch_function( &instruction.0, &instruction.1, &args )
            .map_err(|_| FunctionCallError::InvalidSocket )?;

        load_errors.iter().for_each(| err | crate::utils::produce_warning( err ));

        Ok( results )

    })() {
        Ok( results ) => build_response( &mut caller, results ),
        Err( err ) => send_error( &mut caller, err ),
    }
}

#[inline] fn parse_instruction(
    caller: &mut Caller<Plugin>,
    ptr: WasmMemSegPtr,
    size: WasmMemSegSize,
) -> Result<( InterfaceId, String ), FunctionCallError> {

    let instruction_bytes = extract_wasm_args!( caller, ptr, size )
        .map_err(| _ | FunctionCallError::InvalidInstructionMemorySegment )?;
    let instruction_reader = serialize::read_message( Cursor::new( instruction_bytes ), ReaderOptions::new())
        .map_err(| _ | FunctionCallError::InvalidInstructionData )?;
    let instruction_data = instruction_reader.get_root::<function_call_instruction::Reader>()
        .map_err(| _ | FunctionCallError::InvalidInstructionData )?;
    
    let instruction_socket = instruction_data.get_socket()
        .map_err(| _ | FunctionCallError::InvalidInstructionData )?
        .get_id();
    let instruction_function = instruction_data.get_function()
        .map_err(| _ | FunctionCallError::InvalidInstructionData )?
        .to_string().map_err(| _ | FunctionCallError::InvalidInstructionData )?;

    Ok(( instruction_socket, instruction_function ))

}

#[inline] fn build_response(
    caller: &mut Caller<Plugin>,
    results: Vec<Result<Vec<u8>, DispatchError>>
) -> RawMemorySegment {

    let mut message = message::Builder::new_default();
    let message_root = message.init_root::<function_call_result::Builder>();

    let mut list_builder = message_root.init_result().init_success( results.len() as u32 );

    results.into_iter()
        .enumerate()
        .for_each(|( index, result )| {
            let mut response_builder = list_builder.reborrow()
                .get( index as u32 )
                .init_response();
            match result {
                Ok( success ) => response_builder.set_result( &success ),
                Err( failure ) => match failure {
                    DispatchError::Deadlock => response_builder.set_deadlock(()),
                    // UNREACHABLE: DataTooLarge is only caused
                    // if size can no longer be represented as u32
                    // which considering the data was succesfuly read
                    // cannot happen
                    DispatchError::MemoryWriteError( MemorySendError::DataTooLarge { .. }) => unreachable!(),
                    DispatchError::NoOrInvalidSignature { .. } => response_builder.set_malformed(()),
                    DispatchError::MemoryWriteError(
                        MemorySendError::NoOrInvalidAllocExport { .. }
                        | MemorySendError::PluginAllocException { .. }
                        | MemorySendError::MissingMemoryExport { .. }
                        | MemorySendError::OutOfBoundsMemory { .. }
                    ) => response_builder.set_malformed(()),
                    DispatchError::MemoryReadError(
                        MemoryReadError::NoOrInvalidDeallocExport { .. }
                        | MemoryReadError::PluginDeallocException { .. }
                        | MemoryReadError::MissingMemoryExport { .. }
                        | MemoryReadError::OutOfBoundsMemory { .. }
                    ) => response_builder.set_malformed(()),
                    DispatchError::DispatchException( exception ) => response_builder.set_exception( exception.to_string().as_bytes() ), // TODO
                }
            }
        });

    send_return( caller, message )

}


#[inline] fn send_error(
    caller: &mut Caller<Plugin>,
    error_type: FunctionCallError,
) -> RawMemorySegment {

    let mut message = message::Builder::new_default();
    let message_root = message.init_root::<function_call_result::Builder>();
    message_root.init_result().set_failure( error_type );

    send_return( caller, message )

}

const FAILED_TO_WRITE_RESPONSE_ERROR: RawMemorySegment = 0 ;

#[inline] fn send_return(
    caller: &mut Caller<Plugin>,
    builder: message::Builder<HeapAllocator>,
) -> RawMemorySegment {

    let mut buffer = Vec::new();
    if let Err( err ) = serialize::write_message( &mut buffer, &builder ) {
        panic!( "Failed to write capnp message: {}", err );
    }

    match caller.send_data( &buffer ) {
        Ok( memory_segment ) => memory_segment.as_send(),
        Err( _ ) => FAILED_TO_WRITE_RESPONSE_ERROR,
    }

}