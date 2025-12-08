use wasmtime::{ Engine, Linker, Module, Store };

use super::{ ActivePlugin, PluginTreeNode };
use super::super::Plugin ;



impl PluginTreeNode {
    pub fn load(
        &mut self,
        engine: &Engine,
        linker: &Linker<Plugin>
    ) -> Result<&mut ActivePlugin, wasmtime::Error> {
        match self {
            PluginTreeNode::ActivePlugin( active_plugin ) => Ok( active_plugin ),
            PluginTreeNode::LazyPlugin { .. } => unsafe {

                let mut manually_drop_self = std::mem::ManuallyDrop::new( std::ptr::read( self ));
                let lazy_plugin = match std::mem::ManuallyDrop::take( &mut manually_drop_self ) {
                    PluginTreeNode::LazyPlugin( plugin) => plugin,
                    _ => unreachable!(),
                };

                match load_plugin( &engine, lazy_plugin, &linker ) {
                    Ok( active_plugin ) => {
                        std::ptr::write( self, PluginTreeNode::ActivePlugin( active_plugin ));
                    }
                    Err(( plugin, err )) => {
                        std::ptr::write( self, PluginTreeNode::LazyPlugin( plugin ));
                        return Err( err )
                    }
                }
                
                match self {
                    PluginTreeNode::ActivePlugin( active_plugin ) => Ok( active_plugin ),
                    _ => unreachable!(),
                }

            }
        }
    }
}

#[inline] pub(super) fn load_plugin(
    engine: &Engine,
    plugin: Plugin,
    linker: &Linker<Plugin>
) -> Result<ActivePlugin, ( Plugin, wasmtime::Error )> {
    
    let id = plugin.id().to_owned();

    let module = match Module::from_file( engine, plugin.wasm() ) {
        Ok( module ) => module,
        Err( err ) => return Err(( plugin, err )),
    };

    let mut store = Store::new( engine, plugin );
    let instance = match linker.instantiate( &mut store, &module ) {
        Ok( instance ) => instance,
        Err( err ) => return Err(( store.into_data(), err )),
    };

    Ok( ActivePlugin {
        _id: id,
        _module: module,
        store,
        instance,
    })

}