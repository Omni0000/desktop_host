use std::collections::HashMap ;
use std::sync::{ Arc, RwLock };
use wasmtime::{ Engine, Store };
use wasmtime::component::{ Component, Linker, ResourceType };

use crate::utils::Merge ;
use crate::InterfaceId ;
use super::{ RawPluginData, RawInterfaceData, FunctionData };
use super::{ Socket, PluginInstance, PluginContext, preload_socket, SocketState, PreloadResult, ResourceWrapper, PreloadError };



#[inline] pub fn preload_plugin(
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
    mut plugin: RawPluginData,
) -> PreloadResult< PluginInstance > {
    
    let socket_ids = match plugin.get_sockets() {
        Ok( ids ) => ids,
        Err( err ) => return PreloadResult { socket_map, result: Err( PreloadError::CorruptedPluginManifest( err ) ), errors: Vec::with_capacity( 0 ) }
    };

    let ( socket_map, sockets, errors ) = match preload_child_sockets( &socket_ids, socket_map, engine, default_linker ) {
        PreloadResult { socket_map, result: Ok( sockets ), errors } => ( socket_map, sockets, errors ),
        PreloadResult { socket_map, result: Err( err ), errors } => return PreloadResult { socket_map, result: Err( err ), errors },
    };

    let linker = match sockets.into_iter().try_fold(
        default_linker.clone(),
        | linker, ( interface, socket )| link_socket( linker, interface, socket ),
    ) {
        Ok( linker ) => linker,
        Err( err ) => return PreloadResult { socket_map, result: Err( err ), errors },
    };

    let component = match Component::from_file( linker.engine(), plugin.wasm_path() ) {
        Ok( component ) => component,
        Err( err ) => return PreloadResult { socket_map, result: Err( PreloadError::FailedToLoadComponent( err )), errors },
    };

    let plugin_id = plugin.id().clone();
    let host_data = PluginContext::new( plugin );
    let mut store = Store::new( engine, host_data );
    let instance = match linker.instantiate( &mut store, &component ) {
        Ok( instanace_pre ) => instanace_pre,
        Err( err ) => return PreloadResult { socket_map, result: Err( PreloadError::FailedToLoadComponent( err )), errors },
    };

    let lazy_plugin = PluginInstance {
        id: plugin_id,
        _component: component,
        store,
        instance,
    };

    PreloadResult { socket_map, result: Ok( lazy_plugin ), errors }

}

#[inline] fn preload_child_sockets(
    socket_ids: &Vec<InterfaceId>,
    socket_map: HashMap<InterfaceId, SocketState>,
    engine: &Engine,
    default_linker: &Linker<PluginContext>,
) -> PreloadResult< Vec<(
    Arc<RawInterfaceData>,
    Arc<Socket<RwLock<PluginInstance>>>,
)> > {

    match socket_ids.iter().try_fold(
        ( socket_map, Vec::<( _, _)>::new(), Vec::<PreloadError>::new() ),
        |( socket_map, sockets, errors ), socket_id | match preload_socket( socket_map, engine, default_linker, *socket_id ) {
            PreloadResult { socket_map, result: Ok( socket ), errors: new_errors } =>
                Ok(( socket_map, sockets.merge(( socket.0, socket.1 )), errors.merge_all( new_errors ) )),
            PreloadResult { socket_map, result: Err( err ), errors: new_errors } =>
                Err(( socket_map, err, errors.merge_all( new_errors ))),
        }
    ) {
        Ok(( socket_map, sockets, errors )) => PreloadResult { socket_map, result: Ok( sockets ), errors },
        Err(( socket_map, err, errors )) => PreloadResult { socket_map, result: Err( err ), errors },
    }

}

#[inline] fn link_socket(
    mut linker: Linker<PluginContext>,
    interface: Arc<RawInterfaceData>,
    socket: Arc<Socket<RwLock<PluginInstance>>>,
) -> Result<Linker<PluginContext>, PreloadError> {

    let package = interface.get_package().clone();
    let interface_ident = format!( "{}/root", package );

    let mut root = linker.root();
    let mut linker_instance = root.instance( &interface_ident )
        .map_err(| err | PreloadError::FailedToLinkRootInterface( err ))?;

    interface.get_functions().into_iter().try_for_each(| function: &FunctionData | -> Result<(), PreloadError> {
            
        let function_clone: FunctionData = function.clone();
        let interface_ident_clone = interface_ident.clone();
        let socket_arc_clone = Arc::clone( &socket );

        macro_rules! link {( $dispatch: expr ) => {
            linker_instance.func_new( function.name(), move | ctx, _ty, args, results | Ok(
                results[0] = $dispatch( &socket_arc_clone, ctx, &interface_ident_clone, &function_clone, args )
            )).map_err(| err | PreloadError::FailedToLink( function.name().to_string(), err ) )
        }}

        match function.is_method() {
            false => link!( Socket::dispatch_function_all ),
            true => link!( Socket::dispatch_function_method ),
        }

    })?;

    interface.get_resources().iter().try_for_each(| resource | linker_instance
        .resource( resource, ResourceType::host::<Arc<ResourceWrapper>>(), ResourceWrapper::drop )
        .map_err(| err | PreloadError::FailedToLink( resource.to_string(), err ))
    )?;

    Ok( linker )

}
